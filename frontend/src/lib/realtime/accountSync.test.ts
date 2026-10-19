import { QueryClient } from '@tanstack/react-query'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useAccountSyncStore, type AccountSnapshot } from '@/stores/accountSyncStore'
import { useConversationActivityStore, selectConversationStatus } from '@/stores/conversationActivityStore'
import { useMessageStore } from '@/stores/messageStore'
import { ApiError } from '@/lib/api-v2/client'
const mock = vi.hoisted(() => ({ request: vi.fn(), subscribe: vi.fn(), localRuns: new Set<string>(), localContinuationThreads: new Set<string>(), controller: new AbortController(), origin: '', sse: vi.fn(), notify: vi.fn() }))
vi.mock('@/lib/replyNotifications', () => ({ notifyReplyOutcome: mock.notify }))
vi.mock('./client', async original => ({ ...await original<typeof import('./client')>(), realtimeClient: () => mock }))
vi.mock('@microsoft/fetch-event-source', () => ({ fetchEventSource: mock.sse }))
import { startAccountSync } from './accountSync'

let cleanup: (() => void) | undefined
function snapshot(cursor = '10'): AccountSnapshot {
  return { server_id: 'server', cursor, groups: [{ id: 'group', name: 'Other device', conversation_kind: 'group' }],
    threads: [{ id: 'thread', group_id: 'group', title: 'Task', status: 'active' }], pending_approvals: [],
    active_jobs: [{ id: 'job', stream_id: 'stream', turn_id: 'turn', group_id: 'group', thread_id: 'thread', status: 'running' }] }
}
beforeEach(() => {
  mock.request.mockReset().mockResolvedValue(snapshot()); mock.subscribe.mockReset().mockReturnValue(vi.fn()); mock.sse.mockReset().mockReturnValue(new Promise(() => undefined))
  mock.localRuns.clear(); mock.localContinuationThreads.clear(); mock.controller = new AbortController(); mock.origin = ''
  mock.notify.mockReset()
  useAccountSyncStore.setState(useAccountSyncStore.getInitialState(), true)
  useConversationActivityStore.setState(useConversationActivityStore.getInitialState(), true)
  useMessageStore.setState(useMessageStore.getInitialState(), true)
})
afterEach(() => { cleanup?.(); cleanup = undefined })

it('discovers an unopened conversation from a snapshot and subscribes after its watermark', async () => {
  const queries = new QueryClient()
  const invalidate = vi.spyOn(queries, 'invalidateQueries')
  cleanup = startAccountSync('token', queries)
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  expect(mock.subscribe.mock.calls[0].slice(0, 2)).toEqual(['account', '10'])
  expect(selectConversationStatus(useConversationActivityStore.getState(), 'group')).toBe('running')
  expect(invalidate).toHaveBeenCalled()
  mock.request.mockResolvedValue({ ...snapshot('11'), active_jobs: [] })
  const apply = mock.subscribe.mock.calls[0][2]
  apply({ type: 'event', scope: 'account', cursor: '11', event_id: 'event', kind: 'job.changed', group_id: 'group', thread_id: 'thread', resource_id: 'job', payload: { status: 'succeeded' } })
  apply({ type: 'event', scope: 'account', cursor: '11', event_id: 'event', kind: 'job.changed' })
  await vi.waitFor(() => expect(useAccountSyncStore.getState().snapshot?.cursor).toBe('11'))
  expect(selectConversationStatus(useConversationActivityStore.getState(), 'group')).toBeNull()
})

it('takes a fresh snapshot after an expired account cursor and ignores disposed callbacks', async () => {
  cleanup = startAccountSync('token', new QueryClient())
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  mock.request.mockResolvedValue(snapshot('30'))
  mock.subscribe.mock.calls[0][3](new ApiError(409, 'resync_required', 'Expired'))
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalledTimes(2))
  expect(mock.subscribe.mock.calls[1].slice(0, 2)).toEqual(['account', '30'])
  const late = mock.subscribe.mock.calls[1][2]
  cleanup(); cleanup = undefined
  late({ type: 'event', cursor: '31', event_id: 'late', kind: 'group.changed', group_id: 'group' })
  expect(useAccountSyncStore.getState().snapshot).toBeNull()
  expect(mock.request).toHaveBeenCalledTimes(2)
})

it('does not duplicate the local stream lifecycle', async () => {
  mock.localRuns.add('stream')
  cleanup = startAccountSync('token', new QueryClient())
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  expect(useConversationActivityStore.getState().runs).toEqual({})
})

it('announces a fast remote job once even when it was absent from the snapshot', async () => {
  mock.request.mockResolvedValue({ ...snapshot(), active_jobs: [] })
  cleanup = startAccountSync('token', new QueryClient())
  await vi.waitFor(() => expect(mock.subscribe).toHaveBeenCalled())
  const apply = mock.subscribe.mock.calls[0][2]
  const event = { type: 'event', cursor: '11', event_id: 'finished', kind: 'job.changed', resource_id: 'fast-job',
    group_id: 'group', thread_id: 'thread', payload: { status: 'succeeded', stream_id: 'fast-stream' } }
  apply(event); apply(event)
  expect(mock.notify).toHaveBeenCalledOnce()
  expect(useConversationActivityStore.getState().runs).toEqual({})
})

it('uses account SSE for the saved Noise pairing with the same applied cursor', async () => {
  mock.origin = 'https://qunica-lan.invalid'
  cleanup = startAccountSync('token', new QueryClient())
  await vi.waitFor(() => expect(mock.sse).toHaveBeenCalled())
  const [url, init] = mock.sse.mock.calls[0]
  expect(url).toContain('/account/events?server_id=server&cursor=10')
  init.onmessage({ data: JSON.stringify({ type: 'event', cursor: '11', event_id: 'next', kind: 'message.changed', group_id: 'group', thread_id: 'thread' }), event: '' })
  expect(mock.subscribe).not.toHaveBeenCalled()
  expect(useAccountSyncStore.getState().status).toBe('connected')
  init.onmessage({ event: 'resync_required' })
  await vi.waitFor(() => expect(mock.sse).toHaveBeenCalledTimes(2))
})
