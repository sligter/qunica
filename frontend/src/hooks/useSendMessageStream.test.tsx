import type { ReactNode } from 'react'
import { act, renderHook } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import type { ApiV2SseHandlers } from '@/lib/api-v2/sse'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

const mocks = vi.hoisted(() => ({
  streams: [] as Array<{
    handlers: ApiV2SseHandlers
    url: string
    body: unknown
    abort: ReturnType<typeof vi.fn>
  }>,
  fetchJson: vi.fn(),
  streamStartError: null as unknown,
}))

vi.mock('@/lib/api-v2/client', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-v2/client')>()
  return { ...original, fetchJson: mocks.fetchJson }
})

vi.mock('@/lib/api-v2/sse', () => ({
  openApiV2SseStream: (options: { handlers: ApiV2SseHandlers; url: string; body: unknown }) => {
    if (mocks.streamStartError) throw mocks.streamStartError
    const abort = vi.fn()
    mocks.streams.push({
      handlers: options.handlers,
      url: options.url,
      body: options.body,
      abort,
    })
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
      trigger_message_id: 'message-1',
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

function persistedMessage(id: string, groupId = 'group-1'): Message {
  return {
    id,
    group_id: groupId,
    thread_id: `${groupId}-thread`,
    sender_type: 'user',
    sender_id: 'user-1',
    message_type: 'text',
    content: id,
    attachments: [],
    status: 'visible',
    refs: null,
    context_usage: null,
    turn_id: null,
    dispatch_id: null,
    reply_to_message_id: null,
    turn_summary: null,
    created_at: '2026-07-15T00:00:00Z',
  }
}

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

function emit(handlers: ApiV2SseHandlers, event: unknown) {
  act(() => handlers.onEvent(event as never))
}

function ignoreSend(promise: Promise<void>) {
  void promise.catch(() => undefined)
}

function emitActiveSchedulerStream(
  handlers: ApiV2SseHandlers,
  streamId: string,
  messageId: string,
  agentId: string,
  threadId = 'thread-1',
) {
  emit(handlers, {
    stream_id: streamId,
    seq: 1,
    event_id: `${streamId}-event-1`,
    kind: 'user_message',
    payload: { message_id: messageId, thread_id: threadId, content: messageId },
  })
  emit(handlers, {
    stream_id: streamId,
    seq: 2,
    event_id: `${streamId}-event-2`,
    kind: 'turn_started',
    payload: { turn_id: `${streamId}-turn`, budget },
  })
  emit(handlers, {
    stream_id: streamId,
    seq: 3,
    event_id: `${streamId}-event-3`,
    kind: 'agent_start',
    payload: { agent_id: agentId, display_name: agentId },
  })
  emit(handlers, {
    stream_id: streamId,
    seq: 4,
    event_id: `${streamId}-event-4`,
    kind: 'token',
    payload: { agent_id: agentId, delta: 'working' },
  })
  emit(handlers, {
    stream_id: streamId,
    seq: 5,
    event_id: `${streamId}-event-5`,
    kind: 'tool_call_start',
    payload: {
      agent_id: agentId,
      display_name: agentId,
      tool_call_id: `${streamId}-tool`,
      tool_name: 'lookup',
      status: 'started',
    },
  })
}

describe('useSendMessageStream scheduler events', () => {
  beforeEach(() => {
    vi.useRealTimers()
    mocks.streams.length = 0
    mocks.fetchJson.mockReset()
    mocks.fetchJson.mockResolvedValue(traceResponse())
    mocks.streamStartError = null
    useMessageStore.setState(initialMessages, true)
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
  })

  it('sends its stable local request id and selected task to the backend', () => {
    const queryClient = new QueryClient()
    const hook = renderHook(
      () => useSendMessageStream('group-1', { threadId: 'thread-1' }),
      { wrapper: wrapper(queryClient) },
    )

    act(() => ignoreSend(hook.result.current.send('hello')))

    expect(mocks.streams[0]?.body).toMatchObject({
      content: 'hello',
      attachments: [],
      client_request_id: expect.any(String),
      thread_id: 'thread-1',
    })
  })

  it('resolves send on the persisted user-message acknowledgement before agents finish', async () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let sendPromise!: Promise<void>
    act(() => {
      sendPromise = hook.result.current.send('hello')
    })
    let resolved = false
    void sendPromise.then(() => {
      resolved = true
    })
    await Promise.resolve()
    expect(resolved).toBe(false)

    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })

    await expect(sendPromise).resolves.toBeUndefined()
    expect(hook.result.current.activeStreamCount).toBe(1)
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'agent_start',
      payload: { agent_id: 'agent-1', display_name: 'Agent One' },
    })
    expect(hook.result.current.activeStreamCount).toBe(1)
  })

  it('echoes the message and opens its run before the server acknowledges it', async () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let sendPromise!: Promise<void>
    act(() => {
      sendPromise = hook.result.current.send('hello')
    })

    // Nothing has come back from the server yet, but the conversation already
    // shows the message and the turn that is starting.
    const echoed = useMessageStore.getState().byGroup['group-1'] ?? []
    expect(echoed.map((message) => message.content)).toEqual(['hello'])
    expect(useMessageStore.getState().pendingMessageIds.has(echoed[0].id)).toBe(true)
    const requestId = (mocks.streams[0]?.body as { client_request_id: string }).client_request_id
    expect(useMessageStore.getState().streamRunsByGroup['group-1']?.[requestId]?.status).toBe(
      'active',
    )

    emit(mocks.streams[0].handlers, {
      stream_id: requestId,
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })
    await expect(sendPromise).resolves.toBeUndefined()

    // The persisted row replaces the echo in place — no duplicate, and the run
    // follows the message to its server id.
    const state = useMessageStore.getState()
    expect((state.byGroup['group-1'] ?? []).map((message) => message.id)).toEqual(['message-1'])
    expect(state.pendingMessageIds.size).toBe(0)
    expect(state.streamRunIdByUserMessageIdByGroup['group-1']).toEqual({
      'message-1': requestId,
    })
    expect(state.streamRunsByGroup['group-1']?.[requestId]?.user_message_id).toBe('message-1')
  })

  it('rejects send when the stream reports an error before acknowledgement', async () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let sendPromise!: Promise<void>
    act(() => {
      sendPromise = hook.result.current.send('hello')
    })
    const rejection = expect(sendPromise).rejects.toThrow('persist failed')

    emit(mocks.streams[0].handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'error',
      payload: { message: 'persist failed' },
    })

    await rejection
    expect(hook.result.current.error).toBe('persist failed')
    // A message that never reached the conversation must not keep claiming it
    // did: the echo is taken back with the rejection.
    expect(useMessageStore.getState().byGroup['group-1'] ?? []).toEqual([])
    expect(useMessageStore.getState().pendingMessageIds.size).toBe(0)
  })


  it('keeps send resolved when an agent error arrives after acknowledgement', async () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let sendPromise!: Promise<void>
    act(() => {
      sendPromise = hook.result.current.send('hello')
    })
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })
    await expect(sendPromise).resolves.toBeUndefined()

    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'error',
      payload: { message: 'agent failed after persistence' },
    })

    await expect(sendPromise).resolves.toBeUndefined()
    expect(hook.result.current.error).toBe('agent failed after persistence')
  })

  it('rejects unacknowledged sends on transport error, close, and startup failure', async () => {
    const queryClient = new QueryClient()
    const first = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let transportPromise!: Promise<void>
    act(() => {
      transportPromise = first.result.current.send('transport')
    })
    const transportRejection = expect(transportPromise).rejects.toThrow('network down')
    act(() => mocks.streams[0].handlers.onError?.(new Error('network down')))
    await transportRejection
    first.unmount()

    const second = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let closePromise!: Promise<void>
    act(() => {
      closePromise = second.result.current.send('close')
    })
    const closeRejection = expect(closePromise).rejects.toThrow(
      'Message stream ended before the user message was acknowledged',
    )
    act(() => mocks.streams[1].handlers.onClose?.())
    await closeRejection
    second.unmount()

    mocks.streamStartError = new Error('startup failed')
    const third = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    let startupPromise!: Promise<void>
    act(() => {
      startupPromise = third.result.current.send('startup')
    })
    await expect(startupPromise).rejects.toThrow('startup failed')
    expect(mocks.streams).toHaveLength(2)
  })

  it('queues pre-turn cancellation, posts once when the turn arrives, then aborts', async () => {
    vi.useFakeTimers()
    let resolveCancel!: (value: unknown) => void
    mocks.fetchJson.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveCancel = resolve
      }),
    )
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })

    act(() => ignoreSend(hook.result.current.send('hello')))
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })

    let cancelPromise!: Promise<void>
    act(() => {
      cancelPromise = hook.result.current.cancel()
    })
    expect(mocks.fetchJson).not.toHaveBeenCalled()
    expect(stream.abort).not.toHaveBeenCalled()

    await act(async () => vi.advanceTimersByTimeAsync(6_000))
    expect(mocks.fetchJson).not.toHaveBeenCalled()
    expect(stream.abort).not.toHaveBeenCalled()

    act(() => ignoreSend(hook.result.current.send('blocked while cancelling')))
    expect(mocks.streams).toHaveLength(1)
    expect(hook.result.current.error).toBe('Cancellation is in progress')

    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'speaker_selected',
      payload: {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-1',
        source_agent_id: null,
        target_agent_id: 'agent-1',
        reason: 'deterministic_order',
        action_kind: 'speak',
        hop: 0,
      },
    })
    expect(mocks.fetchJson).toHaveBeenCalledWith('/groups/group-1/turns/turn-1/cancel', {
      method: 'POST',
      token: 'token-1',
    })
    expect(mocks.fetchJson).toHaveBeenCalledTimes(1)
    expect(stream.abort).not.toHaveBeenCalled()

    await act(async () => {
      resolveCancel(traceResponse())
      await cancelPromise
    })
    expect(stream.abort).toHaveBeenCalledTimes(1)
    expect(
      useMessageStore.getState().streamRunsByGroup['group-1']['stream-1'],
    ).toMatchObject({
      status: 'cancelled',
      scheduler_status: 'cancelled',
      terminal_reason: 'user_cancelled',
    })
  })

  it('promotes scheduler waiting_for_user before done and invalidates its trace', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => ignoreSend(hook.result.current.send('hello')))
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'waiting_for_user',
      payload: { agent_id: 'agent-1', message: 'Need approval' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 4,
      event_id: 'event-4',
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

  it('keeps the question and choices from a waiting_for_user event', () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => ignoreSend(hook.result.current.send('hello')))
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'waiting_for_user',
      payload: {
        agent_id: 'agent-1',
        message: 'Waiting for your input',
        input_request: {
          question: 'Shall I fix the Tavily key first?',
          required: true,
          choices: ['Yes, fix it', 'Skip for now'],
        },
      },
    })

    // Without this the timeline renders a bare "Waiting for your input" badge
    // and the user never sees what they were asked.
    const run = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    const notice = run.events.find((event) => event.type === 'waiting_for_user')
    expect(notice).toMatchObject({
      input_request: {
        question: 'Shall I fix the Tavily key first?',
        required: true,
        choices: ['Yes, fix it', 'Skip for now'],
      },
    })
  })

  it('reconciles an idempotent completed cancel response before aborting', async () => {
    mocks.fetchJson.mockResolvedValueOnce(traceResponse('completed', null))
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => ignoreSend(hook.result.current.send('hello')))
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })

    await act(async () => hook.result.current.cancel())

    expect(stream.abort).toHaveBeenCalledTimes(1)
    expect(
      useMessageStore.getState().streamRunsByGroup['group-1']['stream-1'],
    ).toMatchObject({
      status: 'completed',
      scheduler_status: 'completed',
      terminal_reason: null,
    })
  })

  it('keeps active streams alive across unmount and hands them to the remounted chat', () => {
    useMessageStore.getState().setHistory('group-1', [persistedMessage('persisted')])
    const queryClient = new QueryClient()
    const firstHook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => {
      ignoreSend(firstHook.result.current.send('first'))
      ignoreSend(firstHook.result.current.send('second'))
    })
    const [first, second] = mocks.streams
    emitActiveSchedulerStream(first.handlers, 'stream-1', 'message-1', 'agent-1')
    emitActiveSchedulerStream(second.handlers, 'stream-2', 'message-2', 'agent-2')

    firstHook.unmount()

    expect(first.abort).not.toHaveBeenCalled()
    expect(second.abort).not.toHaveBeenCalled()
    expect(mocks.fetchJson).not.toHaveBeenCalled()
    expect(useMessageStore.getState().byGroup['group-1'].map((message) => message.id)).toEqual([
      'persisted',
      'message-1',
      'message-2',
    ])
    expect(useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']).toMatchObject({
      status: 'active',
    })

    const remounted = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    expect(remounted.result.current.isStreaming).toBe(true)
    expect(remounted.result.current.activeStreamCount).toBe(1)

    emit(second.handlers, {
      stream_id: 'stream-2',
      seq: 6,
      event_id: 'stream-2-event-6',
      kind: 'token',
      payload: { agent_id: 'agent-2', delta: '-continued' },
    })
    expect(
      useMessageStore.getState().inFlightByGroup['group-1']['stream-2:agent-2'].content,
    ).toBe('working-continued')

    emit(first.handlers, {
      stream_id: 'stream-1',
      seq: 6,
      event_id: 'stream-1-event-6',
      kind: 'done',
      payload: { turn_id: 'stream-1-turn' },
    })
    expect(remounted.result.current.isStreaming).toBe(true)
    emit(second.handlers, {
      stream_id: 'stream-2',
      seq: 7,
      event_id: 'stream-2-event-7',
      kind: 'done',
      payload: { turn_id: 'stream-2-turn' },
    })
    expect(remounted.result.current.isStreaming).toBe(false)
  })

  it('keeps a background stream isolated and cancellable after navigation', async () => {
    const store = useMessageStore.getState()
    store.setHistory('group-1', [persistedMessage('group-1-history')])
    store.setHistory('group-2', [persistedMessage('group-2-history', 'group-2')])
    const queryClient = new QueryClient()
    const firstHook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => ignoreSend(firstHook.result.current.send('first')))
    const first = mocks.streams[0]
    emitActiveSchedulerStream(first.handlers, 'stream-1', 'message-1', 'agent-1')
    firstHook.unmount()

    const secondHook = renderHook(() => useSendMessageStream('group-2'), {
      wrapper: wrapper(queryClient),
    })

    expect(secondHook.result.current.isStreaming).toBe(false)
    expect(first.abort).not.toHaveBeenCalled()
    expect(mocks.fetchJson).not.toHaveBeenCalled()
    emit(first.handlers, {
      stream_id: 'stream-1',
      seq: 6,
      event_id: 'stream-1-event-6',
      kind: 'token',
      payload: { agent_id: 'agent-1', delta: '-continued' },
    })
    const state = useMessageStore.getState()
    expect(state.inFlightByGroup['group-1']['stream-1:agent-1'].content).toBe(
      'working-continued',
    )
    expect(state.byGroup['group-2'].map((message) => message.id)).toEqual([
      'group-2-history',
    ])

    const returned = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    await act(async () => returned.result.current.cancel())
    expect(first.abort).toHaveBeenCalledTimes(1)
    expect(returned.result.current.isStreaming).toBe(false)
  })

  it('keeps simultaneous task streams isolated and independently controllable', async () => {
    const store = useMessageStore.getState()
    store.setHistory('thread-1', [])
    store.setHistory('thread-2', [])
    const queryClient = new QueryClient()
    const firstHook = renderHook(
      () => useSendMessageStream('group-1', { threadId: 'thread-1' }),
      { wrapper: wrapper(queryClient) },
    )
    act(() => ignoreSend(firstHook.result.current.send('first task')))
    const first = mocks.streams[0]
    emitActiveSchedulerStream(first.handlers, 'stream-1', 'message-1', 'agent-1', 'thread-1')
    firstHook.unmount()

    const secondHook = renderHook(
      () => useSendMessageStream('group-1', { threadId: 'thread-2' }),
      { wrapper: wrapper(queryClient) },
    )
    expect(secondHook.result.current.isStreaming).toBe(false)
    act(() => ignoreSend(secondHook.result.current.send('second task')))
    const second = mocks.streams[1]
    emitActiveSchedulerStream(second.handlers, 'stream-2', 'message-2', 'agent-2', 'thread-2')

    const active = useMessageStore.getState()
    expect(active.byGroup['thread-1'].map((message) => message.id)).toEqual(['message-1'])
    expect(active.byGroup['thread-2'].map((message) => message.id)).toEqual(['message-2'])
    expect(active.streamRunsByGroup['thread-1']['stream-1'].group_id).toBe('group-1')
    expect(active.activeSendsByGroup['thread-1']).toBeDefined()
    expect(active.activeSendsByGroup['thread-2']).toBeDefined()

    await act(async () => secondHook.result.current.cancel())
    expect(second.abort).toHaveBeenCalledTimes(1)
    expect(first.abort).not.toHaveBeenCalled()

    const returned = renderHook(
      () => useSendMessageStream('group-1', { threadId: 'thread-1' }),
      { wrapper: wrapper(queryClient) },
    )
    expect(returned.result.current.isStreaming).toBe(true)
    await act(async () => returned.result.current.cancel())
    expect(first.abort).toHaveBeenCalledTimes(1)
  })

  it('rejects late bubbles and messages after supersede and invalidates the terminal trace', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => ignoreSend(hook.result.current.send('hello')))
    const stream = mocks.streams[0]
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'event-1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'turn_started',
      payload: { turn_id: 'turn-1', budget },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
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
      seq: 4,
      event_id: 'event-4',
      kind: 'token',
      payload: { agent_id: 'agent-1', delta: 'stale' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 5,
      event_id: 'event-5',
      kind: 'agent_message',
      payload: { message_id: 'agent-message-1', agent_id: 'agent-1', content: 'stale' },
    })

    const state = useMessageStore.getState()
    expect(state.byGroup['group-1'].map((message) => message.id)).toEqual(['message-1'])
    expect(state.inFlightByGroup['group-1'] ?? {}).toEqual({})
    expect(state.streamRunsByGroup['group-1']['stream-1'].criticalSummaries).toEqual([
      expect.objectContaining({ kind: 'superseded' }),
    ])
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'turns', 'turn-1'],
    })
  })

  it('refreshes live trace data without storing dispatch details', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => ignoreSend(hook.result.current.send('hello')))
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
        dispatch_id: 'dispatch-private',
        source_agent_id: 'agent-1',
        target_agent_id: 'agent-2',
        reason: 'agent_call',
        action_kind: 'call',
        hop: 1,
      },
    })
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'turns', 'turn-1'],
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'event-3',
      kind: 'turn_budget_exhausted',
      payload: {
        turn_id: 'turn-1',
        status: 'failure_budget_exhausted',
        reason: 'failure_budget_exhausted',
        budget: {
          agent_steps: 3,
          moderator_calls: 0,
          consecutive_failures: 2,
          total_failures: 4,
          total_tokens: 100,
        },
      },
    })

    const run = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    expect(run.criticalSummaries.map((summary) => summary.kind)).toEqual([
      'call',
      'budget_exhausted',
    ])
    expect(run.criticalSummaries.every((summary) => !('dispatch_id' in summary))).toBe(true)
  })
})

describe('useSendMessageStream direct conversations', () => {
  beforeEach(() => {
    mocks.streams.length = 0
    mocks.streamStartError = null
    useMessageStore.setState(initialMessages, true)
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
  })

  it('streams through the direct-chat route and forwards metadata updates', () => {
    const queryClient = new QueryClient()
    const onConversationUpdated = vi.fn()
    const { result } = renderHook(
      () =>
        useSendMessageStream('chat-1', {
          scope: 'direct-chats',
          onConversationUpdated,
        }),
      { wrapper: wrapper(queryClient) },
    )

    act(() => ignoreSend(result.current.send('hello')))

    expect(mocks.streams[0]?.url).toBe('/api/v2/direct-chats/chat-1/messages/stream')
    emit(mocks.streams[0]!.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'stream-1:1',
      kind: 'conversation_updated',
      payload: {
        conversation_id: 'chat-1',
        title: 'Hello',
        title_source: 'automatic',
        updated_at: '2026-07-19T00:00:00Z',
      },
    })

    expect(onConversationUpdated).toHaveBeenCalledWith({
      conversation_id: 'chat-1',
      title: 'Hello',
      title_source: 'automatic',
      updated_at: '2026-07-19T00:00:00Z',
    })
  })

  it('invalidates direct-chat workspace files for agent, tool, ACP, and close events', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const { result } = renderHook(
      () => useSendMessageStream('chat-1', { scope: 'direct-chats' }),
      { wrapper: wrapper(queryClient) },
    )

    act(() => ignoreSend(result.current.send('hello')))
    const stream = mocks.streams[0]!
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 1,
      event_id: 'stream-1:1',
      kind: 'user_message',
      payload: { message_id: 'message-1', thread_id: 'thread-1', content: 'hello' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'stream-1:2',
      kind: 'agent_message',
      payload: { message_id: 'agent-message-1', agent_id: 'agent-1', content: 'reply' },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 3,
      event_id: 'stream-1:3',
      kind: 'tool_call_result',
      payload: {
        agent_id: 'agent-1',
        tool_call_id: 'tool-1',
        tool_name: 'write_file',
        status: 'completed',
      },
    })
    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 4,
      event_id: 'stream-1:4',
      kind: 'acp_agent_run',
      payload: {
        run_id: 'run-1',
        agent_id: 'agent-1',
        display_name: 'Agent One',
        status: 'completed',
      },
    })
    act(() => stream.handlers.onClose?.())

    const directWorkspaceInvalidations = invalidate.mock.calls.filter(
      ([filters]) => filters?.queryKey?.join('/') === 'direct-chats/chat-1/workspace-files',
    )
    expect(directWorkspaceInvalidations).toHaveLength(4)
    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: ['groups', 'chat-1', 'workspace-files'],
    })
  })
})
