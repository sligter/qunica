import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useRef, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TurnTraceDrawer } from '@/components/chat/TurnTraceDrawer'
import i18n from '@/i18n'
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

  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('localizes trace metrics without changing raw diagnostic detail', async () => {
    mocks.trace = { ...mocks.trace, data: traceFixture() }
    await i18n.changeLanguage('en-US')
    renderDrawer()
    expect(screen.getByText('Turn details')).toBeVisible()
    expect(screen.getByLabelText('Turn usage')).toHaveTextContent('Steps2')
    expect(screen.getByLabelText('Turn usage')).toHaveTextContent('Hops2')
    expect(screen.getByLabelText('Turn usage')).toHaveTextContent('Tokens1,234')

    cleanup()
    mocks.trace = { ...mocks.trace, isError: true, error: new Error('RAW_BACKEND_DETAIL') }
    await i18n.changeLanguage('zh-CN')
    renderDrawer()
    expect(screen.getByText('回合详情')).toBeVisible()
    expect(screen.getByRole('alert')).toHaveTextContent('RAW_BACKEND_DETAIL')
  })

  it('localizes known termination reasons and cost while preserving unknown reasons', async () => {
    const localized = traceFixture()
    localized.turn.termination_reason = 'user_cancelled'
    localized.estimated_cost = { amount: '1234.5', currency: 'USD' }
    mocks.trace = { ...mocks.trace, data: localized }
    renderDrawer()
    expect(screen.getByText('User cancelled')).toBeVisible()
    expect(screen.getByText('1,234.5 USD')).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    renderDrawer()
    expect(screen.getByText('用户取消')).toBeVisible()
    expect(screen.getByText('1,234.5 USD')).toBeVisible()

    cleanup()
    const unknown = traceFixture()
    ;(unknown.turn as { termination_reason: string | null }).termination_reason =
      'RAW_TERMINATION_DETAIL'
    mocks.trace = { ...mocks.trace, data: unknown }
    renderDrawer()
    expect(screen.getByText('RAW_TERMINATION_DETAIL')).toBeVisible()
  })

  it('shows a loading state', () => {
    mocks.trace = { ...mocks.trace, isLoading: true }
    renderDrawer()
    expect(screen.getByRole('status')).toHaveTextContent('Loading trace…')
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
    expect(mocks.mutate).toHaveBeenCalledWith({
      groupId: 'group-1',
      turnId: 'turn-1',
    })
  })

  it('keeps a waiting turn cancellable with its exact group and turn target', async () => {
    const user = userEvent.setup()
    const waiting = traceFixture()
    waiting.turn.status = 'waiting_for_user'
    mocks.trace = { ...mocks.trace, data: waiting }
    renderDrawer()

    await user.click(screen.getByRole('button', { name: 'Stop turn' }))
    expect(mocks.mutate).toHaveBeenCalledWith({
      groupId: 'group-1',
      turnId: 'turn-1',
    })
  })

  it('restores focus to the exact button that opened the drawer', async () => {
    const user = userEvent.setup()
    mocks.trace = { ...mocks.trace, data: traceFixture() }

    function Harness() {
      const [open, setOpen] = useState(false)
      const triggerRef = useRef<HTMLButtonElement | null>(null)
      return (
        <>
          <button type="button">First trace</button>
          <button ref={triggerRef} type="button" onClick={() => setOpen(true)}>
            Second trace
          </button>
          <TurnTraceDrawer
            groupId="group-1"
            turnId="turn-1"
            open={open}
            onOpenChange={setOpen}
            returnFocusRef={triggerRef}
          />
        </>
      )
    }

    render(<Harness />)
    const trigger = screen.getByRole('button', { name: 'Second trace' })
    await user.click(trigger)
    await user.keyboard('{Escape}')

    expect(trigger).toHaveFocus()
  })
})
