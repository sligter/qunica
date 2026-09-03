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
