import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useAuthStore } from '@/stores/authStore'
import { parseSchedulerStreamEvent } from '@/lib/api-v2/schemas'
const mock = vi.hoisted(() => ({ command: vi.fn(), request: vi.fn(), subscribe: vi.fn(), sse: vi.fn(), origin: '', localRuns: new Set<string>(), localContinuationThreads: new Set<string>() }))
vi.mock('./client', () => ({ realtimeClient: () => mock }))
vi.mock('@/lib/runtime', () => ({ apiOrigin: () => mock.origin }))
vi.mock('@/lib/api-v2/sse', () => ({ openApiV2SseStream: mock.sse }))
import { openConversationStream } from './stream'

beforeEach(() => {
  mock.origin = ''; mock.command.mockReset(); mock.subscribe.mockReset(); mock.request.mockReset(); mock.sse.mockReset()
  mock.subscribe.mockReturnValue(vi.fn())
  mock.sse.mockReturnValue(new AbortController())
  useAuthStore.setState({ token: 'token' })
})
afterEach(() => useAuthStore.setState({ token: null }))

it('sends once, then observes the accepted stream through the shared WS', async () => {
  mock.command.mockResolvedValue({ request_id: 'request', stream_id: 'accepted-stream' })
  const onEvent = vi.fn()
  const ctrl = openConversationStream({ url: '/api/v2/groups/group/messages/stream', token: 'token', body: { content: 'once', client_request_id: 'request', thread_id: 'thread' }, handlers: { onEvent } })
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  expect(mock.command).toHaveBeenCalledExactlyOnceWith({ request_id: 'request', name: 'message.send', target: { conversation_id: 'group', thread_id: 'thread' }, payload: { content: 'once', client_request_id: 'request', thread_id: 'thread' } })
  expect(mock.subscribe.mock.calls[0].slice(0, 2)).toEqual(['run:accepted-stream', '-1'])
  ctrl.abort()
  mock.subscribe.mock.calls[0][2]({ type: 'event', kind: 'token' })
  expect(onEvent).not.toHaveBeenCalled()
  expect(mock.sse).not.toHaveBeenCalled()
})

it('hydrates an observed run and subscribes at the snapshot cursor without an execution request', async () => {
  const onEvent = vi.fn()
  const initial = { stream_id: 'stream', seq: 4, event_id: 'stream:4', kind: 'token' as const, payload: { agent_id: 'agent', text: 'already generated' } }
  const ctrl = openConversationStream({ url: '/api/v2/groups/group/messages/stream', token: 'token', body: {}, observe: { streamId: 'stream', cursor: '9', events: [initial] }, handlers: { onEvent } })
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  expect(onEvent).toHaveBeenCalledWith(initial)
  expect(mock.subscribe.mock.calls[0].slice(0, 2)).toEqual(['run:stream', '9'])
  expect(mock.command).not.toHaveBeenCalled()
  ctrl.abort()
})

it('passes WS scheduler events through the existing strict domain parser', async () => {
  mock.command.mockResolvedValue({ stream_id: 'accepted-stream' })
  const onEvent = vi.fn(event => expect(parseSchedulerStreamEvent(event)?.kind).toBe('moderator_started'))
  const ctrl = openConversationStream({ url: '/api/v2/groups/group/messages/stream', token: 'token', body: { client_request_id: 'request' }, handlers: { onEvent } })
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  mock.subscribe.mock.calls[0][2]({ type: 'event', scope: 'run:accepted-stream', cursor: '3',
    stream_id: 'accepted-stream', seq: 3, event_id: 'accepted-stream:3', kind: 'moderator_started', payload: { turn_id: 'turn' } })
  expect(onEvent).toHaveBeenCalledOnce()
  ctrl.abort()
})

it('binds approvals to the immutable pending action and current version', async () => {
  mock.request.mockResolvedValue({ pending_approvals: [{ tool_call_id: 'call', action_hash: 'hash', version: 3 }] })
  mock.command.mockResolvedValue({ stream_id: 'continued' })
  const ctrl = openConversationStream({ url: '/api/v2/threads/thread/resume', token: 'token', body: { approval: { tool_call_id: 'call', approved: true } }, handlers: { onEvent: vi.fn() } })
  await vi.waitFor(() => expect(mock.command).toHaveBeenCalled())
  expect(mock.command.mock.calls[0][0]).toMatchObject({ name: 'approval.resolve', target: { thread_id: 'thread' }, payload: { tool_call_id: 'call', approved: true, action_hash: 'hash', expected_version: 3 } })
  ctrl.abort()
})

it('retains the paired Fetch stream adapter', () => {
  mock.origin = 'https://qunica-lan.invalid'
  const opts = { url: '/send', token: 'token', body: {}, handlers: { onEvent: vi.fn() } }
  openConversationStream(opts)
  expect(mock.sse).toHaveBeenCalledWith(expect.objectContaining({ url: opts.url, token: opts.token, body: opts.body }))
  expect(mock.command).not.toHaveBeenCalled()
})

it('observes a paired run without a negative SSE event ID before its first event', () => {
  mock.origin = 'https://qunica-lan.invalid'
  const controller = openConversationStream({ url: '/send', token: 'token', body: {},
    replayUrl: () => '/api/v2/groups/group/streams/stream',
    observe: { streamId: 'stream', cursor: '-1', events: [] }, handlers: { onEvent: vi.fn() } })
  expect(mock.sse).toHaveBeenCalledWith(expect.objectContaining({
    url: '/api/v2/groups/group/streams/stream', initialMethod: 'GET', lastEventId: undefined,
  }))
  expect(mock.command).not.toHaveBeenCalled()
  controller.abort()
})

it('resumes an observed paired run from its applied snapshot cursor', () => {
  mock.origin = 'https://qunica-lan.invalid'
  const controller = openConversationStream({ url: '/send', token: 'token', body: {},
    replayUrl: () => '/api/v2/groups/group/streams/stream',
    observe: { streamId: 'stream', cursor: '12', events: [] }, handlers: { onEvent: vi.fn() } })
  expect(mock.sse).toHaveBeenCalledWith(expect.objectContaining({ initialMethod: 'GET', lastEventId: 'stream:12' }))
  expect(mock.command).not.toHaveBeenCalled()
  controller.abort()
})
