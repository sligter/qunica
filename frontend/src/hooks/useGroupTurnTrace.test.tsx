import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import type { PropsWithChildren } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { fetchJson } from '@/lib/api-v2/client'
import type { GroupTurnTraceResponse } from '@/lib/api-v2/types'
import {
  groupTurnTraceQueryKey,
  useCancelGroupTurn,
  useGroupTurnTrace,
} from '@/hooks/useGroupTurnTrace'
import { useAuthStore } from '@/stores/authStore'

vi.mock('@/lib/api-v2/client', () => ({ fetchJson: vi.fn() }))

const mockedFetchJson = vi.mocked(fetchJson)

function traceFixture(status: GroupTurnTraceResponse['turn']['status'] = 'running'): GroupTurnTraceResponse {
  return {
    turn: {
      id: 'turn-1',
      thread_id: 'thread-1',
      group_id: 'group-1',
      trigger_message_id: 'message-1',
      status,
      scheduler_strategy: 'bounded_deterministic',
      config_snapshot: {},
      topology_snapshot: {},
      agent_steps: 1,
      moderator_calls: 0,
      consecutive_failures: 0,
      total_failures: 0,
      total_tokens: 42,
      termination_reason: status === 'cancelled' ? 'user_cancelled' : null,
      created_at: '2026-07-15T00:00:00Z',
      started_at: '2026-07-15T00:00:01Z',
      completed_at: status === 'pending' || status === 'running' || status === 'waiting_for_user'
        ? null
        : '2026-07-15T00:00:02Z',
      updated_at: '2026-07-15T00:00:02Z',
    },
    budget: {
      agent_steps: 1,
      moderator_calls: 0,
      consecutive_failures: 0,
      total_failures: 0,
      total_tokens: 42,
    },
    dispatches: [],
    estimated_cost: null,
    cost_estimation_status: 'unavailable',
  }
}

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

describe('useGroupTurnTrace', () => {
  beforeEach(() => {
    mockedFetchJson.mockReset()
    useAuthStore.setState({ token: 'owner-token' })
  })

  afterEach(() => {
    cleanup()
    useAuthStore.setState({ token: null })
  })

  it('fetches an owner-scoped trace and strictly parses the response', async () => {
    mockedFetchJson.mockResolvedValueOnce(traceFixture())
    const client = testClient()
    const { result } = renderHook(() => useGroupTurnTrace('group-1', 'turn-1'), {
      wrapper: wrapper(client),
    })

    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(mockedFetchJson).toHaveBeenCalledWith('/groups/group-1/turns/turn-1', {
      token: 'owner-token',
    })
    expect(result.current.data?.turn.id).toBe('turn-1')
  })

  it('polls active traces and stops after a terminal response', async () => {
    vi.useFakeTimers()
    try {
      mockedFetchJson
        .mockResolvedValueOnce(traceFixture('waiting_for_user'))
        .mockResolvedValueOnce(traceFixture('completed'))
      const client = testClient()
      renderHook(() => useGroupTurnTrace('group-1', 'turn-1'), {
        wrapper: wrapper(client),
      })

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1)
      })
      expect(mockedFetchJson).toHaveBeenCalledTimes(1)

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2_000)
      })
      expect(mockedFetchJson).toHaveBeenCalledTimes(2)

      await act(async () => {
        await vi.advanceTimersByTimeAsync(4_000)
      })
      expect(mockedFetchJson).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('rejects a malformed trace instead of leaking unchecked data', async () => {
    mockedFetchJson.mockResolvedValueOnce({ ...traceFixture(), reasoning: 'private' })
    const client = testClient()
    const { result } = renderHook(() => useGroupTurnTrace('group-1', 'turn-1'), {
      wrapper: wrapper(client),
    })

    await waitFor(() => expect(result.current.isError).toBe(true))
  })

  it('posts cancellation and replaces the trace cache only after success', async () => {
    const cancelled = traceFixture('cancelled')
    mockedFetchJson.mockResolvedValueOnce(cancelled)
    const client = testClient()
    client.setQueryData(groupTurnTraceQueryKey('group-1', 'turn-1'), traceFixture())
    const { result } = renderHook(() => useCancelGroupTurn(), {
      wrapper: wrapper(client),
    })

    await act(async () => {
      await result.current.mutateAsync({ groupId: 'group-1', turnId: 'turn-1' })
    })

    expect(mockedFetchJson).toHaveBeenCalledWith('/groups/group-1/turns/turn-1/cancel', {
      method: 'POST',
      token: 'owner-token',
    })
    expect(client.getQueryData<GroupTurnTraceResponse>(groupTurnTraceQueryKey('group-1', 'turn-1'))?.turn.status).toBe('cancelled')
  })

  it('caches cancellation under the request snapshot and invalidates its messages', async () => {
    let resolveRequest: ((trace: GroupTurnTraceResponse) => void) | undefined
    mockedFetchJson.mockImplementationOnce(() => new Promise((resolve) => {
      resolveRequest = resolve
    }))
    const client = testClient()
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    const { result } = renderHook(() => useCancelGroupTurn(), {
      wrapper: wrapper(client),
    })

    let cancellation!: Promise<unknown>
    act(() => {
      cancellation = result.current.mutateAsync({ groupId: 'group-1', turnId: 'turn-1' })
    })
    await waitFor(() => expect(resolveRequest).toBeTypeOf('function'))
    resolveRequest?.(traceFixture('cancelled'))
    await act(async () => {
      await cancellation
    })

    expect(client.getQueryData<GroupTurnTraceResponse>(
      groupTurnTraceQueryKey('group-1', 'turn-1'),
    )?.turn.status).toBe('cancelled')
    expect(client.getQueryData(groupTurnTraceQueryKey('group-2', 'turn-2'))).toBeUndefined()
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'group-1', 'messages'],
    })
  })
})
