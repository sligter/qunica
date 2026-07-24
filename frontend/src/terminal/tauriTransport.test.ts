import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: (event: unknown) => void }>,
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class ChannelMock {
    onmessage = () => undefined

    constructor() {
      tauriMock.channels.push(this)
    }
  },
  invoke: tauriMock.invoke,
}))

import {
  decodeBase64Bytes,
  createTauriTerminalTransport,
} from './tauriTransport'
import {
  createUnavailableTerminalTransport,
  TerminalTransportError,
} from './transport'

describe('createTauriTerminalTransport', () => {
  beforeEach(() => {
    tauriMock.channels.length = 0
    tauriMock.invoke.mockReset().mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('creates a session with the exact request and ordered channel callback', async () => {
    const descriptor = {
      sessionId: 'session-1',
      shellName: 'PowerShell',
      cwd: 'D:/project',
    }
    tauriMock.invoke.mockResolvedValueOnce(descriptor)
    const onEvent = vi.fn()
    const transport = createTauriTerminalTransport()

    await expect(
      transport.create(
        { conversationId: 'chat-1', cwd: 'D:/project', cols: 80, rows: 24 },
        onEvent,
      ),
    ).resolves.toEqual(descriptor)

    const channel = tauriMock.channels[0]
    expect(channel).toBeDefined()
    expect(tauriMock.invoke).toHaveBeenCalledWith('terminal_create', {
      request: { conversationId: 'chat-1', cwd: 'D:/project', cols: 80, rows: 24 },
      onEvent: channel,
    })

    channel?.onmessage({
      event: 'output',
      data: { bytesBase64: 'AP+A8J+YgA==' },
    })
    channel?.onmessage({
      event: 'exit',
      data: { code: 7, signal: null },
    })
    channel?.onmessage({
      event: 'error',
      data: { code: 'terminal.read_failed', message: 'Read failed' },
    })

    expect(onEvent).toHaveBeenNthCalledWith(1, {
      event: 'output',
      data: { bytes: new Uint8Array([0, 255, 128, 240, 159, 152, 128]) },
    })
    expect(onEvent).toHaveBeenNthCalledWith(2, {
      event: 'exit',
      data: { code: 7, signal: null },
    })
    expect(onEvent).toHaveBeenNthCalledWith(3, {
      event: 'error',
      data: { code: 'terminal.read_failed', message: 'Read failed' },
    })
  })

  it('uses exact payloads for a small write, resize, close, and closeAll', async () => {
    const transport = createTauriTerminalTransport()

    await transport.write('chat-1', 'session-1', 'pwd\r')
    await transport.resize('chat-1', 'session-1', 120, 40)
    await transport.close('chat-1', 'session-1')
    await transport.closeAll()

    expect(tauriMock.invoke.mock.calls).toEqual([
      [
        'terminal_write',
        { conversationId: 'chat-1', sessionId: 'session-1', data: 'pwd\r' },
      ],
      [
        'terminal_resize',
        { conversationId: 'chat-1', sessionId: 'session-1', cols: 120, rows: 40 },
      ],
      ['terminal_close', { conversationId: 'chat-1', sessionId: 'session-1' }],
      ['terminal_close_all'],
    ])
  })

  it('preserves an empty write as one IPC invocation', async () => {
    await createTauriTerminalTransport().write('chat-1', 'session-1', '')

    expect(tauriMock.invoke).toHaveBeenCalledOnce()
    expect(tauriMock.invoke).toHaveBeenCalledWith('terminal_write', {
      conversationId: 'chat-1',
      sessionId: 'session-1',
      data: '',
    })
  })

  it('writes large ASCII input in bounded chunks and awaits each invoke in order', async () => {
    let releaseFirst: (() => void) | undefined
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirst = resolve
    })
    tauriMock.invoke
      .mockImplementationOnce(() => firstWrite)
      .mockResolvedValueOnce(undefined)
    const input = 'a'.repeat(16 * 1024 + 9)

    const writing = createTauriTerminalTransport().write('chat-1', 'session-1', input)
    expect(tauriMock.invoke).toHaveBeenCalledTimes(1)
    expect(tauriMock.invoke.mock.calls[0]?.[1]).toEqual({
      conversationId: 'chat-1',
      sessionId: 'session-1',
      data: 'a'.repeat(16 * 1024),
    })

    releaseFirst?.()
    await writing

    expect(tauriMock.invoke).toHaveBeenCalledTimes(2)
    expect(tauriMock.invoke.mock.calls[1]?.[1]).toEqual({
      conversationId: 'chat-1',
      sessionId: 'session-1',
      data: 'a'.repeat(9),
    })
  })

  it('does not split a Unicode code point that crosses the 16 KiB boundary', async () => {
    const input = `${'a'.repeat(16 * 1024 - 1)}😀b`

    await createTauriTerminalTransport().write('chat-1', 'session-1', input)

    const chunks = tauriMock.invoke.mock.calls.map(
      (call) => (call[1] as { data: string }).data,
    )
    expect(chunks).toEqual(['a'.repeat(16 * 1024 - 1), '😀b'])
    expect(chunks.join('')).toBe(input)
    for (const chunk of chunks) {
      expect(new TextEncoder().encode(chunk).byteLength).toBeLessThanOrEqual(16 * 1024)
    }
  })

  it('replaces isolated surrogates before writing ordered UTF-8-bounded chunks', async () => {
    let releaseFirst: (() => void) | undefined
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirst = resolve
    })
    tauriMock.invoke
      .mockImplementationOnce(() => firstWrite)
      .mockResolvedValueOnce(undefined)
    const input = `${'a'.repeat(16 * 1024 - 3)}\uD800${'b'.repeat(16 * 1024 - 4)}\uDC00c`
    const firstChunk = `${'a'.repeat(16 * 1024 - 3)}\ufffd`
    const secondChunk = `${'b'.repeat(16 * 1024 - 4)}\ufffdc`

    const writing = createTauriTerminalTransport().write(
      'chat-1',
      'session-1',
      input,
    )

    expect(tauriMock.invoke).toHaveBeenCalledTimes(1)
    expect(tauriMock.invoke.mock.calls[0]?.[1]).toEqual({
      conversationId: 'chat-1',
      sessionId: 'session-1',
      data: firstChunk,
    })

    releaseFirst?.()
    await writing

    const chunks = tauriMock.invoke.mock.calls.map(
      (call) => (call[1] as { data: string }).data,
    )
    expect(chunks).toEqual([firstChunk, secondChunk])
    expect(chunks.join('')).toBe(firstChunk + secondChunk)
    expect(chunks.join('')).not.toContain('\uD800')
    expect(chunks.join('')).not.toContain('\uDC00')
    for (const chunk of chunks) {
      expect(new TextEncoder().encode(chunk).byteLength).toBeLessThanOrEqual(16 * 1024)
    }
  })

  it('serializes complete writes for the same session', async () => {
    let releaseFirst: (() => void) | undefined
    const firstInvoke = new Promise<void>((resolve) => {
      releaseFirst = resolve
    })
    tauriMock.invoke
      .mockImplementationOnce(() => firstInvoke)
      .mockResolvedValue(undefined)
    const transport = createTauriTerminalTransport()
    const firstInput = 'a'.repeat(16 * 1024 + 1)

    const firstWrite = transport.write('chat-1', 'session-1', firstInput)
    const secondWrite = transport.write('chat-1', 'session-1', 'b')

    expect(tauriMock.invoke).toHaveBeenCalledTimes(1)
    expect(tauriMock.invoke.mock.calls[0]?.[1]).toMatchObject({
      sessionId: 'session-1',
      data: 'a'.repeat(16 * 1024),
    })

    releaseFirst?.()
    await Promise.all([firstWrite, secondWrite])

    expect(
      tauriMock.invoke.mock.calls.map((call) =>
        (call[1] as { sessionId: string; data: string }).data,
      ),
    ).toEqual(['a'.repeat(16 * 1024), 'a', 'b'])
  })

  it('continues the same-session queue after a failed write', async () => {
    tauriMock.invoke
      .mockRejectedValueOnce(new Error('first write failed'))
      .mockResolvedValueOnce(undefined)
    const transport = createTauriTerminalTransport()

    const failedWrite = transport.write('chat-1', 'session-1', 'a')
    const nextWrite = transport.write('chat-1', 'session-1', 'b')

    await expect(failedWrite).rejects.toMatchObject({
      code: 'terminal.command_failed',
      message: 'first write failed',
    })
    await expect(nextWrite).resolves.toBeUndefined()
    expect(
      tauriMock.invoke.mock.calls.map((call) =>
        (call[1] as { sessionId: string; data: string }).data,
      ),
    ).toEqual(['a', 'b'])
  })

  it('does not block writes to a different session', async () => {
    let releaseFirstSession: (() => void) | undefined
    const blockedInvoke = new Promise<void>((resolve) => {
      releaseFirstSession = resolve
    })
    tauriMock.invoke.mockImplementation(
      (_command: string, args: { sessionId?: string } | undefined) =>
        args?.sessionId === 'session-a' ? blockedInvoke : Promise.resolve(),
    )
    const transport = createTauriTerminalTransport()

    const firstSessionWrite = transport.write('chat-1', 'session-a', 'a')
    const secondSessionWrite = transport.write('chat-1', 'session-b', 'b')

    expect(tauriMock.invoke).toHaveBeenCalledTimes(2)
    expect(
      tauriMock.invoke.mock.calls.map((call) =>
        (call[1] as { sessionId: string }).sessionId,
      ),
    ).toEqual(['session-a', 'session-b'])

    releaseFirstSession?.()
    await Promise.all([firstSessionWrite, secondSessionWrite])
  })

  it('does not send later chunks when the first write invocation fails', async () => {
    tauriMock.invoke.mockRejectedValueOnce(new Error('write failed'))
    const input = 'a'.repeat(16 * 1024 + 1)

    await expect(
      createTauriTerminalTransport().write('chat-1', 'session-1', input),
    ).rejects.toMatchObject({
      code: 'terminal.command_failed',
      message: 'write failed',
    })

    expect(tauriMock.invoke).toHaveBeenCalledOnce()
    expect(tauriMock.invoke).toHaveBeenCalledWith('terminal_write', {
      conversationId: 'chat-1',
      sessionId: 'session-1',
      data: 'a'.repeat(16 * 1024),
    })
  })

  it('normalizes Rust command error objects', async () => {
    tauriMock.invoke.mockRejectedValueOnce({
      code: 'terminal.session_forbidden',
      message: 'Terminal session belongs to another conversation',
    })

    const promise = createTauriTerminalTransport().resize(
      'chat-b',
      'session-1',
      80,
      24,
    )

    await expect(promise).rejects.toMatchObject({
      name: 'TerminalTransportError',
      code: 'terminal.session_forbidden',
      message: 'Terminal session belongs to another conversation',
    })
    await expect(promise).rejects.toBeInstanceOf(TerminalTransportError)
  })

  it('decodes Base64 without relying on a Node-only API', () => {
    vi.stubGlobal('atob', undefined)
    expect(decodeBase64Bytes('AP+A8J+YgA==')).toEqual(
      new Uint8Array([0, 255, 128, 240, 159, 152, 128]),
    )
  })
})

describe('unavailable terminal transport', () => {
  beforeEach(() => {
    tauriMock.invoke.mockClear()
  })

  it('rejects every operation without invoking Tauri IPC', async () => {
    const transport = createUnavailableTerminalTransport()
    const expected = {
      code: 'terminal.desktop_required',
      message: 'Terminal is available only in the desktop app.',
    }

    await expect(
      transport.create(
        { conversationId: 'chat-1', cwd: 'D:/project', cols: 80, rows: 24 },
        vi.fn(),
      ),
    ).rejects.toMatchObject(expected)
    await expect(transport.write('chat-1', 'session-1', 'pwd')).rejects.toMatchObject(expected)
    await expect(transport.resize('chat-1', 'session-1', 80, 24)).rejects.toMatchObject(expected)
    await expect(transport.close('chat-1', 'session-1')).rejects.toMatchObject(expected)
    await expect(transport.closeAll()).rejects.toMatchObject(expected)
    expect(tauriMock.invoke).not.toHaveBeenCalled()
  })
})
