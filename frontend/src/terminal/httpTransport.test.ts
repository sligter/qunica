import type { FetchEventSourceInit } from '@microsoft/fetch-event-source'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useAuthStore } from '@/stores/authStore'
import type { TerminalEvent } from '@/terminal/types'

const fetchEventSource = vi.hoisted(() => vi.fn<(
  input: RequestInfo,
  init: FetchEventSourceInit,
) => Promise<void>>(() => Promise.resolve()))
vi.mock('@microsoft/fetch-event-source', () => ({ fetchEventSource }))

import { createHttpTerminalTransport } from './httpTransport'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('createHttpTerminalTransport', () => {
  beforeEach(() => {
    fetchEventSource.mockClear()
    useAuthStore.setState({ token: 'owner-token' })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  async function openTerminal() {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse({
      session_id: 'session-1', shell_name: 'bash', cwd: '/workspace',
    }))
    vi.stubGlobal('fetch', fetchMock)
    const onEvent = vi.fn()
    const transport = createHttpTerminalTransport()
    await transport.create({ conversationId: 'chat-1', cwd: '/workspace', cols: 80, rows: 24 }, onEvent)
    return { transport, onEvent, fetchMock, stream: fetchEventSource.mock.calls[0]![1] }
  }

  function output(id: number) {
    return { id: String(id), event: '', retry: undefined,
      data: JSON.stringify({ event: 'output', data: { bytes_base64: 'aGk=' } }) }
  }

  it('reattaches to the same PTY with the delivered cursor and ignores old callbacks', async () => {
    const { stream, onEvent, fetchMock, transport } = await openTerminal()
    stream.onmessage?.(output(1))
    window.dispatchEvent(new Event('online'))
    document.dispatchEvent(new Event('visibilitychange'))
    await Promise.resolve()
    expect(fetchEventSource).toHaveBeenCalledTimes(2)
    expect(stream.signal?.aborted).toBe(true)
    const [url, restored] = fetchEventSource.mock.calls[1]!
    expect(url).toBe('/api/v2/terminal/sessions/session-1/events')
    expect(new Headers(restored.headers).get('last-event-id')).toBe('1')
    stream.onmessage?.(output(2))
    restored.onmessage?.(output(1))
    expect(onEvent).toHaveBeenCalledOnce()
    restored.onmessage?.(output(2))
    expect(onEvent).toHaveBeenCalledTimes(2)
    expect(fetchMock).toHaveBeenCalledOnce()
    await transport.close('chat-1', 'session-1')
    window.dispatchEvent(new Event('online'))
    await Promise.resolve()
    expect(fetchEventSource).toHaveBeenCalledTimes(2)
  })

  it('retries an unexpected EOF without resending input or advancing an unapplied cursor', async () => {
    const { stream, onEvent, fetchMock } = await openTerminal()
    stream.onmessage?.(output(1))
    onEvent.mockImplementationOnce(() => { throw new Error('renderer unavailable') })
    expect(() => stream.onmessage?.(output(2))).toThrow('renderer unavailable')
    expect(() => stream.onclose?.()).toThrow('before the shell exited')
    expect(() => stream.onerror?.(new TypeError('offline'))).not.toThrow()
    await stream.fetch?.('/events', { headers: { 'last-event-id': '2' } })
    expect(new Headers(fetchMock.mock.calls[1]![1]?.headers).get('last-event-id')).toBe('1')
    expect(fetchMock.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(1)
  })

  it('reports replay gaps and stops reconnecting instead of silently losing output', async () => {
    const { stream, onEvent } = await openTerminal()
    stream.onmessage?.(output(1))
    let failure: unknown
    try { stream.onmessage?.(output(3)) } catch (error) { failure = error }
    expect(failure).toMatchObject({ code: 'terminal.output_gap' })
    expect(() => stream.onerror?.(failure)).toThrow('replay buffer')
    expect(onEvent).toHaveBeenLastCalledWith({ event: 'error', data: {
      code: 'terminal.output_gap', message: expect.stringContaining('replay buffer'),
    } })
    expect(stream.signal?.aborted).toBe(true)
    window.dispatchEvent(new Event('online'))
    await Promise.resolve()
    expect(fetchEventSource).toHaveBeenCalledOnce()
  })

  it('does not resurrect a PTY closed while foreground recovery is queued', async () => {
    const { stream, transport } = await openTerminal()
    window.dispatchEvent(new Event('online'))
    await transport.close('chat-1', 'session-1')
    expect(stream.signal?.aborted).toBe(true)
    expect(fetchEventSource).toHaveBeenCalledOnce()
  })

  it('aborts the body and foreground recovery on sign-out', async () => {
    const { stream, onEvent } = await openTerminal()
    useAuthStore.setState({ token: null })
    stream.onmessage?.(output(1))
    window.dispatchEvent(new Event('online'))
    await Promise.resolve()
    expect(stream.signal?.aborted).toBe(true)
    expect(onEvent).not.toHaveBeenCalled()
    expect(fetchEventSource).toHaveBeenCalledOnce()
  })

  it('creates, streams, writes, resizes, and closes a server PTY', async () => {
    const fetchMock = vi.fn<typeof fetch>().mockImplementation(async (input, init) => {
      const url = String(input)
      if (init?.method === 'POST' && url.endsWith('/terminal/sessions')) {
        return jsonResponse({
          session_id: 'session-1',
          shell_name: 'bash',
          cwd: '/workspaces/demo',
        }, 201)
      }
      return new Response(null, { status: 204 })
    })
    vi.stubGlobal('fetch', fetchMock)
    const events: TerminalEvent[] = []
    const transport = createHttpTerminalTransport()

    await expect(transport.create({
      conversationId: 'chat-1',
      cwd: '/workspaces/demo',
      cols: 100,
      rows: 30,
      shell: 'bash',
    }, (event) => events.push(event))).resolves.toEqual({
      sessionId: 'session-1',
      shellName: 'bash',
      cwd: '/workspaces/demo',
    })

    expect(fetchEventSource).toHaveBeenCalledOnce()
    const [streamUrl, streamOptions] = fetchEventSource.mock.calls[0]!
    expect(streamUrl).toBe('/api/v2/terminal/sessions/session-1/events')
    expect(new Headers(streamOptions.headers).get('Authorization')).toBe('Bearer owner-token')
    streamOptions.onmessage?.({
      data: JSON.stringify({ event: 'output', data: { bytes_base64: 'aGk=' } }),
      event: '',
      id: '1',
      retry: undefined,
    })
    expect(events).toEqual([{
      event: 'output',
      data: { bytes: new Uint8Array([104, 105]) },
    }])

    await transport.write('chat-1', 'session-1', 'echo hi\r')
    await transport.resize('chat-1', 'session-1', 120, 40)
    await transport.close('chat-1', 'session-1')

    expect(streamOptions.signal?.aborted).toBe(true)
    expect(fetchMock.mock.calls.map(([url, init]) => [String(url), init?.method])).toEqual([
      ['/api/v2/terminal/sessions', 'POST'],
      ['/api/v2/terminal/sessions/session-1/input', 'POST'],
      ['/api/v2/terminal/sessions/session-1/resize', 'POST'],
      ['/api/v2/terminal/sessions/session-1', 'DELETE'],
    ])
    for (const [, init] of fetchMock.mock.calls) {
      expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer owner-token')
    }
  })

  it('surfaces backend error envelopes without opening a stream', async () => {
    vi.stubGlobal('fetch', vi.fn<typeof fetch>().mockResolvedValue(jsonResponse({
      error: { code: 'permission_denied', message: 'outside workspace' },
    }, 403)))

    await expect(createHttpTerminalTransport().create({
      conversationId: 'chat-1',
      cwd: '/etc',
      cols: 80,
      rows: 24,
    }, vi.fn())).rejects.toMatchObject({
      code: 'permission_denied',
      message: 'outside workspace',
    })
    expect(fetchEventSource).not.toHaveBeenCalled()
  })
})
