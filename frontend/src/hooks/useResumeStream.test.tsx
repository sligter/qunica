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

function traceResponse(
  status: 'cancelled' | 'completed' = 'cancelled',
  terminationReason: 'user_cancelled' | null = 'user_cancelled',
) {
  return {
    turn: {
      id: 'turn-1',
      thread_id: 'thread-1',
      group_id: 'group-1',
      trigger_message_id: 'trigger-1',
      status,
      scheduler_strategy: 'deterministic',
      config_snapshot: {},
      topology_snapshot: {},
      agent_steps: 1,
      moderator_calls: 0,
      consecutive_failures: 0,
      total_failures: 0,
      total_tokens: 10,
      termination_reason: terminationReason,
      created_at: '2026-07-15T00:00:00Z',
      started_at: '2026-07-15T00:00:01Z',
      completed_at: '2026-07-15T00:00:02Z',
      updated_at: '2026-07-15T00:00:02Z',
    },
    budget: {
      agent_steps: 1,
      moderator_calls: 0,
      consecutive_failures: 0,
      total_failures: 0,
      total_tokens: 10,
    },
    dispatches: [],
    estimated_cost: null,
    cost_estimation_status: 'unavailable',
  }
}

const interruptedMessage: Message = {
  id: 'message-1',
  group_id: 'group-1',
  thread_id: 'thread-1',
  sender_type: 'agent',
  sender_id: 'agent-1',
  message_type: 'text',
  content: 'partial',
  attachments: [],
  status: 'interrupted',
  refs: null,
  context_usage: null,
  turn_id: 'turn-1',
  dispatch_id: 'dispatch-1',
  reply_to_message_id: 'trigger-1',
  turn_summary: null,
  created_at: '2026-07-15T00:00:00Z',
}

const triggerMessage: Message = {
  id: 'trigger-1',
  group_id: 'group-1',
  thread_id: 'thread-1',
  sender_type: 'user',
  sender_id: 'user-1',
  message_type: 'text',
  content: 'Start the task',
  attachments: [],
  status: 'visible',
  refs: null,
  context_usage: null,
  turn_id: 'turn-1',
  dispatch_id: null,
  reply_to_message_id: null,
  turn_summary: { status: 'waiting_for_user', termination_reason: 'waiting_for_user' },
  created_at: '2026-07-14T23:59:59Z',
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
    mocks.fetchJson.mockResolvedValue(traceResponse())
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

  it('replays a pending question when resuming', () => {
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
      kind: 'waiting_for_user',
      payload: {
        agent_id: 'agent-1',
        message: 'Waiting for your input',
        input_request: { question: 'Shall I fix the Tavily key?', required: true },
      },
    })

    // Reloading while an agent waits must not lose what it asked.
    const run = useMessageStore.getState().streamRunsByGroup['group-1']?.['stream-1']
    const notice = run?.events.find((event) => event.type === 'waiting_for_user')
    expect(notice).toMatchObject({
      input_request: { question: 'Shall I fix the Tavily key?' },
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

  it('maps a resumed scheduler run back to the persisted trigger user message', () => {
    useMessageStore.getState().setHistory('group-1', [triggerMessage, interruptedMessage])
    const queryClient = new QueryClient()
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-resumed',
      seq: 1,
      event_id: 'event-1',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-resumed',
      seq: 2,
      event_id: 'event-2',
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
    expect(state.streamRunIdByUserMessageIdByGroup['group-1']['trigger-1']).toBe(
      'stream-resumed',
    )
    expect(state.streamRunsByGroup['group-1']['stream-resumed']).toMatchObject({
      user_message_id: 'trigger-1',
      turn_id: 'turn-1',
      scheduler_status: 'completed',
    })
    expect(state.byGroup['group-1'].map((message) => message.id)).toEqual([
      'trigger-1',
      'message-1',
    ])
  })

  it('registers a legacy resume so shared controls cancel the server and stream', async () => {
    useMessageStore.getState().setHistory('group-1', [
      { ...interruptedMessage, turn_id: null, dispatch_id: null },
    ])
    const queryClient = new QueryClient()
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-resumed',
      seq: 1,
      event_id: 'event-1',
      kind: 'agent_start',
      payload: { agent_id: 'agent-1', display_name: 'Agent One' },
    })

    const registered = useMessageStore.getState().activeResumesByMessageId['message-1']
    expect(registered.group_id).toBe('group-1')
    await act(async () => registered.cancel())

    expect(mocks.fetchJson).toHaveBeenCalledWith('/threads/thread-1/cancel', {
      method: 'POST',
      token: 'token-1',
    })
    expect(stream.abort).toHaveBeenCalledTimes(1)
    expect(
      useMessageStore.getState().activeResumesByMessageId['message-1'],
    ).toBeUndefined()
  })

  it('reconciles waiting_for_user to the parsed cancel response before aborting', async () => {
    useMessageStore.getState().setHistory('group-1', [triggerMessage, interruptedMessage])
    const queryClient = new QueryClient()
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-resumed',
      seq: 1,
      event_id: 'event-1',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-resumed',
      seq: 2,
      event_id: 'event-2',
      kind: 'waiting_for_user',
      payload: { message: 'Need more information' },
    })

    await act(async () => hook.result.current.cancel())

    expect(mocks.fetchJson).toHaveBeenCalledWith('/groups/group-1/turns/turn-1/cancel', {
      method: 'POST',
      token: 'token-1',
    })
    expect(stream.abort).toHaveBeenCalledTimes(1)
    expect(
      useMessageStore.getState().streamRunsByGroup['group-1']['stream-resumed'],
    ).toMatchObject({
      status: 'cancelled',
      scheduler_status: 'cancelled',
      terminal_reason: 'user_cancelled',
    })
  })
})
