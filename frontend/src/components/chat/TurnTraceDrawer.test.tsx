import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TurnTraceDrawer } from '@/components/chat/TurnTraceDrawer'
import type { GroupTurnTraceResponse } from '@/lib/api-v2/types'

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.stubGlobal('ResizeObserver', ResizeObserverMock)

const mocks = vi.hoisted(() => ({
  trace: {} as Record<string, unknown>,
  cancel: {} as Record<string, unknown>,
  refetch: vi.fn(),
  mutate: vi.fn(),
}))

vi.mock('@/hooks/useGroupTurnTrace', () => ({
  useGroupTurnTrace: () => mocks.trace,
  useCancelGroupTurn: () => mocks.cancel,
}))

function traceFixture(): GroupTurnTraceResponse {
  return {
    turn: {
      id: 'turn-1', thread_id: 'thread-1', group_id: 'group-1', trigger_message_id: 'message-1',
      status: 'running', scheduler_strategy: 'bounded_deterministic', config_snapshot: {}, topology_snapshot: {},
      agent_steps: 2, moderator_calls: 0, consecutive_failures: 0, total_failures: 0, total_tokens: 1234,
      termination_reason: null, created_at: '2026-07-15T00:00:00Z', started_at: '2026-07-15T00:00:01Z',
      completed_at: null, updated_at: '2026-07-15T00:00:02Z',
    },
    budget: { agent_steps: 2, moderator_calls: 0, consecutive_failures: 0, total_failures: 0, total_tokens: 1234 },
    dispatches: [{
      id: 'dispatch-1', turn_id: 'turn-1', parent_dispatch_id: null, source_agent_id: null,
      target_agent_id: 'agent-1', selection_reason: 'user_mention', action_kind: 'handoff', hop: 2,
      status: 'running', input_message_id: 'message-1', output_message_id: null,
      artifact: { mode: 'handoff', outcome: 'accepted' }, total_tokens: 1234, failure_code: null,
      created_at: '2026-07-15T00:00:00Z', started_at: '2026-07-15T00:00:01Z', completed_at: null,
      updated_at: '2026-07-15T00:00:02Z',
    }],
    estimated_cost: null,
    cost_estimation_status: 'unavailable',
  }
}

function renderDrawer() {
  return render(
    <TurnTraceDrawer groupId="group-1" turnId="turn-1" open onOpenChange={vi.fn()} />,
  )
}

describe('TurnTraceDrawer', () => {
  beforeEach(() => {
    mocks.refetch.mockReset()
    mocks.mutate.mockReset()
    mocks.trace = { data: undefined, isLoading: false, isError: false, error: null, refetch: mocks.refetch }
    mocks.cancel = { isPending: false, isError: false, error: null, mutate: mocks.mutate }
  })

  afterEach(cleanup)

  it('shows a loading state', () => {
    mocks.trace = { ...mocks.trace, isLoading: true }
    renderDrawer()
    expect(screen.getByRole('status')).toHaveTextContent('Loading trace...')
  })

  it('shows an error and retries the query', async () => {
    const user = userEvent.setup()
    mocks.trace = { ...mocks.trace, isError: true, error: new Error('network unavailable') }
    renderDrawer()
    expect(screen.getByRole('alert')).toHaveTextContent('network unavailable')
    await user.click(screen.getByRole('button', { name: 'Retry' }))
    expect(mocks.refetch).toHaveBeenCalledTimes(1)
  })

  it('renders usage, unavailable cost, public artifacts, and server cancellation', async () => {
    const user = userEvent.setup()
    mocks.trace = { ...mocks.trace, data: traceFixture() }
    renderDrawer()

    expect(screen.getByLabelText('Turn usage')).toHaveTextContent('Steps2')
    expect(screen.getByLabelText('Turn usage')).toHaveTextContent('Hops2')
    expect(screen.getByLabelText('Turn usage')).toHaveTextContent('Tokens1,234')
    expect(screen.getByText('Cost unavailable')).toBeVisible()
    expect(screen.getByText('accepted')).toBeVisible()
    expect(screen.queryByText(/reasoning/i)).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Stop turn' }))
    expect(mocks.mutate).toHaveBeenCalledTimes(1)
  })
})
