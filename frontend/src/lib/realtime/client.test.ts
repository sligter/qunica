import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAuthStore } from '@/stores/authStore'
import { coalesceTextEvents, RealtimeClient, validCursor } from './client'

class Socket {
  static OPEN = 1
  static all: Socket[] = []
  readyState = 1
  bufferedAmount = 0
  frames: Record<string, unknown>[] = []
  onmessage?: (event: { data: string }) => void
  onclose?: () => void
  onerror?: () => void
  constructor(readonly url: URL, readonly protocol: string) { Socket.all.push(this) }
  send(raw: string) { this.frames.push(JSON.parse(raw)) }
  close() { if (this.readyState === 3) return; this.readyState = 3; this.onclose?.() }
  receive(frame: Record<string, unknown>) { this.onmessage?.({ data: JSON.stringify(frame) }) }
  authenticate() {
    this.receive({ type: 'hello', server_id: 'server', protocol: 'qunica.realtime.v2' })
    this.receive({ type: 'authenticated' })
  }
}
const clients: RealtimeClient[] = []
const fetchMock = vi.fn()
function client() { const value = new RealtimeClient('', 'token'); clients.push(value); return value }
async function socket(index = 0) {
  await vi.waitFor(() => expect(Socket.all.length).toBeGreaterThan(index))
  return Socket.all[index]
}

beforeEach(() => {
  Socket.all = []
  vi.stubGlobal('WebSocket', Socket)
  useAuthStore.setState({ token: 'token' })
  fetchMock.mockReset().mockImplementation(async () => new Response(JSON.stringify({ ticket: 'ticket', server_id: 'server', protocol: 'qunica.realtime.v2' })))
  vi.stubGlobal('fetch', fetchMock)
})
afterEach(() => { for (const value of clients.splice(0)) value.dispose(); vi.unstubAllGlobals() })

describe('realtime client', () => {
  it('uses a one-use first-frame ticket and resumes only successfully applied decimal cursors', async () => {
    const connection = client()
    const apply = vi.fn()
    connection.subscribe('account', '9007199254740992', apply, vi.fn())
    const first = await socket()
    expect(first.url.search).toBe('')
    first.authenticate()
    expect(first.frames[0]).toEqual({ type: 'authenticate', ticket: 'ticket' })
    const event = { type: 'event', scope: 'account', cursor: '9007199254740993', event_id: 'a' }
    first.receive(event)
    first.receive(event)
    expect(apply).toHaveBeenCalledOnce()
    apply.mockImplementationOnce(() => { throw new Error('Cannot apply') })
    first.receive({ ...event, cursor: '9007199254740994' })
    window.dispatchEvent(new Event('online'))
    const second = await socket(1)
    second.authenticate()
    expect(second.frames).toContainEqual({ type: 'subscribe', scope: 'account', cursor: '9007199254740993', server_id: 'server' })
    second.receive({ ...event, cursor: '9007199254740994' })
    expect(apply).toHaveBeenCalledTimes(3)
  })

  it('applies a batched frame in order and acknowledges it once', async () => {
    const connection = client()
    const apply = vi.fn()
    connection.subscribe('run:run', '0', apply, vi.fn())
    const first = await socket()
    first.authenticate()
    first.receive({
      type: 'events', scope: 'run:run', cursor: '3',
      events: [1, 2, 3].map(seq => ({ type: 'event', scope: 'run:run', cursor: String(seq), kind: 'token' })),
    })
    expect(apply.mock.calls.map(([event]) => event.cursor)).toEqual(['1', '2', '3'])
    expect(first.frames.filter(frame => frame.type === 'ack')).toEqual([{ type: 'ack', scope: 'run:run', cursor: '3' }])
  })

  it('merges consecutive text deltas per agent without crossing other events', () => {
    const token = (cursor: string, agent: string, delta: string, kind = 'token') =>
      ({ type: 'event', cursor, stream_id: 'run', kind, payload: { agent_id: agent, delta, text: delta } })
    const merged = coalesceTextEvents([
      token('1', 'a', 'he'), token('2', 'a', 'llo'), token('3', 'b', '!'),
      token('4', 'b', '?', 'reasoning'), { type: 'event', cursor: '5', kind: 'tool_call_start' }, token('6', 'b', '.'),
    ])
    expect(merged.map(event => [event.cursor, (event.payload as { delta?: string } | undefined)?.delta])).toEqual([
      ['2', 'hello'], ['3', '!'], ['4', '?'], ['5', undefined], ['6', '.'],
    ])
  })

  it('queries an uncertain command receipt after reconnect and never sends it again', async () => {
    const connection = client()
    const receipt = { status: 'accepted', request_id: 'request', stream_id: 'run' }
    const pending = connection.command({ request_id: 'request', name: 'message.send', target: { conversation_id: 'group' }, payload: { content: 'once' } })
    const first = await socket()
    first.authenticate()
    await vi.waitFor(() => expect(first.frames.some(frame => frame.type === 'command')).toBe(true))
    first.close()
    fetchMock.mockImplementation(async (url: string) => new Response(JSON.stringify(url.endsWith('/commands/request') ? receipt : { ticket: 'ticket2', server_id: 'server', protocol: 'qunica.realtime.v2' })))
    window.dispatchEvent(new Event('online'))
    const second = await socket(1)
    second.authenticate()
    await expect(pending).resolves.toEqual(receipt)
    expect(second.frames.some(frame => frame.type === 'command')).toBe(false)
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/commands/request'))).toBe(true)
  })

  it('invalidates callbacks immediately when the device identity changes', async () => {
    const connection = client()
    const apply = vi.fn()
    connection.subscribe('run:run', '-1', apply, vi.fn())
    const first = await socket()
    first.authenticate()
    useAuthStore.setState({ token: 'another' })
    first.receive({ type: 'event', scope: 'run:run', cursor: '0' })
    expect(apply).not.toHaveBeenCalled()
    expect(first.readyState).toBe(3)
    expect(connection.controller.signal.aborted).toBe(true)
  })

  it('requires explicit client upgrade for an incompatible protocol', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ protocol: 'v2' })))
    const connection = client()
    const failure = vi.fn()
    connection.subscribe('account', '0', vi.fn(), failure)
    await vi.waitFor(() => expect(failure).toHaveBeenCalled())
    expect(Socket.all).toHaveLength(0)
  })

  it('validates cursors without losing 64-bit precision', () => {
    expect(validCursor('9223372036854775807')).toBe(true)
    expect(validCursor('9223372036854775808')).toBe(false)
    expect(validCursor('01')).toBe(false)
    expect(validCursor('-1', true)).toBe(true)
  })
})
