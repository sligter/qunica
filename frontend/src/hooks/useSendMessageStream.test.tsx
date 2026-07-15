import type { ReactNode } from 'react'
import { act, renderHook } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import type { ApiV2SseHandlers } from '@/lib/api-v2/sse'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'

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

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

function emit(handlers: ApiV2SseHandlers, event: unknown) {
  act(() => handlers.onEvent(event as never))
}

describe('useSendMessageStream scheduler events', () => {
  beforeEach(() => {
    mocks.streams.length = 0
    mocks.fetchJson.mockReset()
    mocks.fetchJson.mockResolvedValue({})
    useMessageStore.setState(initialMessages, true)
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
  })

  it('queues pre-turn cancellation, posts once when the turn arrives, then aborts', async () => {
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

    act(() => hook.result.current.send('hello'))
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

    act(() => hook.result.current.send('blocked while cancelling'))
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
      resolveCancel({})
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
    act(() => hook.result.current.send('hello'))
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

  it('cancels a stream as legacy once a legacy execution event identifies it', async () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => hook.result.current.send('hello'))
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
    expect(stream.abort).not.toHaveBeenCalled()

    emit(stream.handlers, {
      stream_id: 'stream-1',
      seq: 2,
      event_id: 'event-2',
      kind: 'agent_start',
      payload: { agent_id: 'agent-1', display_name: 'Agent One' },
    })
    await act(async () => cancelPromise)

    expect(mocks.fetchJson).not.toHaveBeenCalled()
    expect(stream.abort).toHaveBeenCalledTimes(1)
    expect(
      useMessageStore.getState().streamRunsByGroup['group-1']['stream-1'].status,
    ).toBe('cancelled')
  })

  it('rejects late bubbles and messages after supersede and invalidates the terminal trace', () => {
    const queryClient = new QueryClient()
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => hook.result.current.send('hello'))
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

  it('normalizes live call and budget events without storing dispatch details', () => {
    const queryClient = new QueryClient()
    const hook = renderHook(() => useSendMessageStream('group-1'), {
      wrapper: wrapper(queryClient),
    })
    act(() => hook.result.current.send('hello'))
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
