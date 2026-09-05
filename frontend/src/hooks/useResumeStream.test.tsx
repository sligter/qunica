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
  streams: [] as Array<{
    url: string
    handlers: ApiV2SseHandlers
    abort: ReturnType<typeof vi.fn>
  }>,
  fetchJson: vi.fn(),
}))

vi.mock('@/lib/api-v2/client', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-v2/client')>()
  return { ...original, fetchJson: mocks.fetchJson }
})

vi.mock('@/lib/api-v2/sse', () => ({
  openApiV2SseStream: (options: { url: string; handlers: ApiV2SseHandlers }) => {
    const abort = vi.fn()
    mocks.streams.push({ url: options.url, handlers: options.handlers, abort })
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
    useMessageStore.getState().setHistory('thread-1', [interruptedMessage])
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
  })

  it('ignores a completed recovery snapshot after the approval subscription was cancelled', async () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useResumeStream('group-1', 'thread-1', 'message-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]!
    emit(stream.handlers, { stream_id: 'stream-1', seq: 1, event_id: 'event-1',
      kind: 'turn_started', payload: { turn_id: 'turn-1', budget } })
    const before = useMessageStore.getState().streamRunsByGroup['thread-1']['stream-1']
    const trace = traceResponse('completed', null)
    let resolve!: (value: typeof trace) => void
    mocks.fetchJson.mockImplementationOnce(() => new Promise(done => { resolve = done }))
    const ctrl = new AbortController()
    const recovery = stream.handlers.onRecover!(ctrl.signal)
    expect(mocks.fetchJson).toHaveBeenLastCalledWith('/groups/group-1/turns/turn-1', {
      token: 'token-1', signal: ctrl.signal,
    })
    ctrl.abort()
    await act(async () => { resolve(trace); expect(await recovery).toBe(false) })
    expect(useMessageStore.getState().streamRunsByGroup['thread-1']['stream-1']).toEqual(before)
    expect(mocks.streams).toHaveLength(1)
  })

  it('normalizes scheduler events with live-send parity and refreshes live trace data', () => {
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
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'turns', 'turn-1'],
    })
    invalidate.mockClear()
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'token',
      payload: { delta: ' more' },
    })
    expect(useMessageStore.getState().byGroup['thread-1'][0].content).toBe('partial more')
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
    expect(state.byGroup['thread-1'][0]).toMatchObject({
      content: 'final',
      turn_id: 'turn-1',
      dispatch_id: 'dispatch-1',
      reply_to_message_id: 'trigger-1',
    })
    expect(state.streamRunsByGroup['thread-1']['stream-1']).toMatchObject({
      turn_id: 'turn-1',
      scheduler_status: 'completed',
      criticalSummaries: [expect.objectContaining({ kind: 'handoff' })],
    })
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'turns', 'turn-1'],
    })
  })

  it('keeps a resume stream alive and controllable after the chat remounts', () => {
    const queryClient = new QueryClient()
    const firstHook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => firstHook.result.current.resume())
    const stream = mocks.streams[0]
    firstHook.unmount()

    expect(stream.abort).not.toHaveBeenCalled()
    const remounted = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    expect(remounted.result.current.isStreaming).toBe(true)

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
      kind: 'token',
      payload: { delta: ' continued' },
    })
    expect(useMessageStore.getState().byGroup['thread-1'][0].content).toBe(
      'partial continued',
    )
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'done',
      payload: { turn_id: 'turn-1' },
    })
    expect(remounted.result.current.isStreaming).toBe(false)
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
    const run = useMessageStore.getState().streamRunsByGroup['thread-1']?.['stream-1']
    const notice = run?.events.find((event) => event.type === 'waiting_for_user')
    expect(notice).toMatchObject({
      input_request: { question: 'Shall I fix the Tavily key?' },
    })
  })

  it('keeps response segments, reasoning and tool calls from the resumed agent_message event', () => {
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
      kind: 'agent_message',
      payload: {
        message_id: 'message-1',
        agent_id: 'agent-1',
        content: 'first tool result',
        response_segments: ['first', 'after tool'],
        reasoning: ['thinking...'],
        tool_calls: [
          {
            tool_call_id: 'call_1',
            tool_name: 'Read',
            status: 'completed',
            args_summary: 'note.txt',
            result_summary: 'ok',
          },
        ],
      },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'done',
      payload: { turn_id: 'turn-1' },
    })

    const message = useMessageStore.getState().byGroup['thread-1'][0]
    expect(message.content).toBe('first tool result')
    expect(message.response_segments).toEqual(['first', 'after tool'])
    expect(message.reasoning).toEqual(['thinking...'])
    expect(message.tool_calls).toEqual([
      expect.objectContaining({ tool_call_id: 'call_1', tool_name: 'Read' }),
    ])
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

    expect(useMessageStore.getState().byGroup['thread-1'][0].content).toBe('partial')
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

    const run = useMessageStore.getState().streamRunsByGroup['thread-1']['stream-1']
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

  it('maps a resumed scheduler run back to the preceding user message for legacy checkpoints', () => {
    useMessageStore.getState().setHistory('thread-1', [
      triggerMessage,
      { ...interruptedMessage, reply_to_message_id: null },
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
    expect(state.streamRunIdByUserMessageIdByGroup['thread-1']['trigger-1']).toBe(
      'stream-resumed',
    )
    expect(state.streamRunsByGroup['thread-1']['stream-resumed']).toMatchObject({
      user_message_id: 'trigger-1',
      turn_id: 'turn-1',
      scheduler_status: 'completed',
    })
    expect(state.byGroup['thread-1'].map((message) => message.id)).toEqual([
      'trigger-1',
      'message-1',
    ])
  })

  it('shows the card when a resume stops at a fresh approval gate', () => {
    useMessageStore.getState().setHistory('thread-1', [triggerMessage, interruptedMessage])
    const queryClient = new QueryClient()
    const hook = renderHook(
      () => useResumeStream('group-1', 'thread-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]
    // A plain continuation emits no scheduler events, so this run has nothing
    // but the card itself to hang on to.
    emit(stream.handlers, {
      stream_id: 'stream-resumed',
      seq: 1,
      event_id: 'event-1',
      kind: 'approval_required',
      payload: {
        agent_id: 'agent-1',
        display_name: 'Agent One',
        message: 'Approval required to delete files',
        tool_call_id: 'call_rm',
        approval_request: {
          rule: 'delete-files',
          capability: 'delete files in this workspace',
          reason: 'it deletes files',
          subject: 'rm cache.txt',
          tool_name: 'Pwsh',
        },
      },
    })

    const state = useMessageStore.getState()
    const run = state.streamRunsByGroup['thread-1']['stream-resumed']
    expect(run.events).toContainEqual(
      expect.objectContaining({
        type: 'approval_required',
        approval_request: expect.objectContaining({ tool_call_id: 'call_rm' }),
      }),
    )
    // A run is only rendered under the user message it belongs to. Unlinked,
    // the card never reaches the screen and the paused turn can only be
    // answered by pressing continue again — which just re-proposes the command.
    expect(state.streamRunIdByUserMessageIdByGroup['thread-1']['trigger-1']).toBe(
      'stream-resumed',
    )
  })

  it('streams into the conversation bucket even when that is not the resumed thread', () => {
    // The main group conversation is read through the group id while its
    // messages live on a thread of their own. Deriving one from the other put
    // the continuation in a bucket nothing renders, so the bubble stayed on the
    // partial text until a history refetch happened to land.
    useMessageStore.getState().setHistory('group-1', [triggerMessage, interruptedMessage])
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(
      () => useResumeStream('group-1', 'group-1', 'message-1'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    const stream = mocks.streams[0]

    // The thread being continued is the message's own, not the bucket's.
    expect(stream.url).toBe('/api/v2/threads/thread-1/resume')
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'token',
      payload: { delta: ' more' },
    })
    const resumed = useMessageStore
      .getState()
      .byGroup['group-1'].find((message) => message.id === 'message-1')
    expect(resumed?.content).toBe('partial more')

    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'done',
      payload: {},
    })
    // And the history the view actually reads is the one invalidated.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'messages'],
    })
  })

  it('invalidates the history of the container the conversation actually lives in', () => {
    // A direct chat is read through `['direct-chats', id, 'messages']`. Keying
    // the refresh to groups left the resumed message showing its partial text
    // until something unrelated happened to refetch.
    useMessageStore.getState().setHistory('chat-1', [triggerMessage, interruptedMessage])
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(
      () => useResumeStream('chat-1', 'chat-1', 'message-1', 'direct-chats'),
      { wrapper: wrapper(queryClient) },
    )
    act(() => hook.result.current.resume())
    emit(mocks.streams[0].handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'done',
      payload: {},
    })

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'messages'],
    })
    // And nothing group-shaped: a direct chat has no roster query, and firing a
    // group key at it would be a refresh aimed at a cache that does not exist.
    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: ['groups', 'chat-1', 'agents'],
    })
  })

  it('registers a legacy resume so shared controls cancel the server and stream', async () => {
    useMessageStore.getState().setHistory('thread-1', [
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
    expect(registered.state_id).toBe('thread-1')
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
    useMessageStore.getState().setHistory('thread-1', [triggerMessage, interruptedMessage])
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
      useMessageStore.getState().streamRunsByGroup['thread-1']['stream-resumed'],
    ).toMatchObject({
      status: 'cancelled',
      scheduler_status: 'cancelled',
      terminal_reason: 'user_cancelled',
    })
  })
})
