import type { ReactNode } from 'react'
import { act, renderHook } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useResumeStream } from '@/hooks/useResumeStream'
import type { ApiV2SseHandlers } from '@/lib/api-v2/sse'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

const mocks = vi.hoisted(() => ({
  streams: [] as Array<{ handlers: ApiV2SseHandlers; abort: ReturnType<typeof vi.fn> }>,
  fetchJson: vi.fn(),
}))

vi.mock('@/lib/api-v2/client', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-v2/client')>()
  return { ...original, fetchJson: mocks.fetchJson }
})

vi.mock('@/lib/api-v2/sse', () => ({
  openApiV2SseStream: (options: { handlers: ApiV2SseHandlers }) => {
    const abort = vi.fn()
    mocks.streams.push({ handlers: options.handlers, abort })
    return { abort } as unknown as AbortController
  },
}))

const initialMessages = useMessageStore.getInitialState()
const budget = {
  max_agent_steps: 8,
  max_steps_per_agent: 3,
  max_hops: 4,
  max_moderator_calls: 2,
  max_consecutive_failures: 2,
  max_total_failures: 4,
  max_total_tokens: 1000,
}

const interruptedMessage: Message = {
  id: 'message-1',
  group_id: 'group-1',
  thread_id: 'thread-1',
  sender_type: 'agent',
  sender_id: 'agent-1',
  message_type: 'text',
  content: 'partial',
  status: 'interrupted',
  refs: null,
  context_usage: null,
  turn_id: 'turn-1',
  dispatch_id: 'dispatch-1',
  reply_to_message_id: 'trigger-1',
  turn_summary: null,
  created_at: '2026-07-15T00:00:00Z',
}

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

function emit(handlers: ApiV2SseHandlers, event: unknown) {
  act(() => handlers.onEvent(event as never))
}

describe('useResumeStream scheduler events', () => {
  beforeEach(() => {
    mocks.streams.length = 0
    mocks.fetchJson.mockReset()
    mocks.fetchJson.mockResolvedValue({})
    useMessageStore.setState(initialMessages, true)
    useMessageStore.getState().setHistory('group-1', [interruptedMessage])
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
  })

  it('normalizes scheduler events with live-send parity and invalidates terminal trace data', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'speaker_selected',
      payload: {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-2',
        source_agent_id: 'agent-1',
        target_agent_id: 'agent-2',
        reason: 'agent_handoff',
        action_kind: 'handoff',
        hop: 1,
      },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'token',
      payload: { delta: ' more' },
    })
    expect(useMessageStore.getState().byGroup['group-1'][0].content).toBe('partial more')
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 4,
      event_id: 'event-4',
      kind: 'agent_message',
      payload: { message_id: 'message-1', agent_id: 'agent-1', content: 'final' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 5,
      event_id: 'event-5',
      kind: 'turn_completed',
      payload: {
        turn_id: 'turn-1',
        status: 'completed',
        reason: null,
        budget: {
          agent_steps: 2,
          moderator_calls: 0,
          consecutive_failures: 0,
          total_failures: 0,
          total_tokens: 20,
        },
      },
    })

    const state = useMessageStore.getState()
    expect(state.byGroup['group-1'][0]).toMatchObject({
      content: 'final',
      turn_id: 'turn-1',
      dispatch_id: 'dispatch-1',
      reply_to_message_id: 'trigger-1',
    })
    expect(state.streamRunsByGroup['group-1']['stream-1']).toMatchObject({
      turn_id: 'turn-1',
      scheduler_status: 'completed',
      criticalSummaries: [expect.objectContaining({ kind: 'handoff' })],
    })
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'turns', 'turn-1'],
    })
  })

  it('rejects resumed tokens and final messages after the turn is superseded', () => {
    const queryClient = new QueryClient()
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'turn_superseded',
      payload: {
        turn_id: 'turn-1',
        status: 'superseded',
        reason: 'superseded',
        budget: {
          agent_steps: 1,
          moderator_calls: 0,
          consecutive_failures: 0,
          total_failures: 0,
          total_tokens: 10,
        },
      },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'token',
      payload: { delta: ' stale' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 4,
      event_id: 'event-4',
      kind: 'agent_message',
      payload: { message_id: 'message-1', agent_id: 'agent-1', content: 'stale final' },
    })

    expect(useMessageStore.getState().byGroup['group-1'][0].content).toBe('partial')
  })

  it('keeps waiting_for_user status when resume ends with scheduler done', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'waiting_for_user',
      payload: { message: 'Need more information' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'done',
      payload: { turn_id: 'turn-1' },
    })

    const run = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    expect(run).toMatchObject({
      status: 'completed',
      scheduler_status: 'waiting_for_user',
      terminal_reason: 'waiting_for_user',
    })
    expect(run.criticalSummaries).toEqual([
      expect.objectContaining({ kind: 'waiting_for_user' }),
    ])
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'turns', 'turn-1'],
    })
  })
})
