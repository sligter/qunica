import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { TurnSummary } from '@/components/chat/TurnSummary'

describe('TurnSummary', () => {
  afterEach(cleanup)

  it('shows critical status and opens its trace from the keyboard', async () => {
    const user = userEvent.setup()
    const onViewTrace = vi.fn()
    render(
      <TurnSummary
        turnId="turn-1"
        status="failure_budget_exhausted"
        summaries={[{ id: 'failure-1', message: 'Agent call failed', tone: 'destructive' }]}
        onViewTrace={onViewTrace}
      />,
    )

    expect(screen.getByText('Failure budget reached')).toBeVisible()
    expect(screen.getByText('Agent call failed')).toBeVisible()
    await user.tab()
    expect(screen.getByRole('button', { name: 'View trace' })).toHaveFocus()
    await user.keyboard('{Enter}')
    expect(onViewTrace).toHaveBeenCalledWith(
      'turn-1',
      screen.getByRole('button', { name: 'View trace' }),
    )
  })

  it('announces status and keeps the latest two critical summaries', () => {
    render(
      <TurnSummary
        turnId="turn-1"
        status="cancelled"
        summaries={[
          { id: 'old', message: 'Initial route' },
          { id: 'failure', message: 'Agent call failed', tone: 'destructive' },
          { id: 'cancel', message: 'Turn cancelled', tone: 'warning' },
        ]}
        onViewTrace={vi.fn()}
      />,
    )

    expect(screen.getByRole('status')).toHaveTextContent('Cancelled')
    expect(screen.queryByText('Initial route')).not.toBeInTheDocument()
    expect(screen.getByText('Agent call failed')).toBeVisible()
    expect(screen.getByText('Turn cancelled')).toBeVisible()
  })
})
