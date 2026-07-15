import { cleanup, render, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { DispatchDag } from '@/components/chat/DispatchDag'
import type { AgentDispatchTrace } from '@/lib/api-v2/types'

function dispatch(
  id: string,
  parent_dispatch_id: string | null,
  target_agent_id = id,
): AgentDispatchTrace {
  return {
    id,
    turn_id: 'turn-1',
    parent_dispatch_id,
    source_agent_id: null,
    target_agent_id,
    selection_reason: 'deterministic_order',
    action_kind: 'speak',
    hop: parent_dispatch_id ? 1 : 0,
    status: 'completed',
    input_message_id: null,
    output_message_id: null,
    artifact: null,
    total_tokens: 12,
    failure_code: null,
    created_at: '2026-07-15T00:00:00Z',
    started_at: '2026-07-15T00:00:01Z',
    completed_at: '2026-07-15T00:00:02Z',
    updated_at: '2026-07-15T00:00:02Z',
  }
}

describe('DispatchDag', () => {
  afterEach(cleanup)

  it('builds parent-child adjacency in stable dispatch order', () => {
    render(<DispatchDag dispatches={[
      dispatch('root', null),
      dispatch('child-a', 'root'),
      dispatch('child-b', 'root'),
    ]} />)
    const rootBranch = screen.getByText('root').closest('li')
    expect(rootBranch).not.toBeNull()
    const childList = rootBranch?.querySelector('ul')
    expect(childList).not.toBeNull()
    expect(within(childList!).getAllByText(/child-/).map((node) => node.textContent)).toEqual([
      'child-a',
      'child-b',
    ])
  })

  it('renders orphan and cycle records once without recursing forever', () => {
    render(
      <DispatchDag dispatches={[
        dispatch('orphan', 'missing', 'Orphan agent'),
        dispatch('cycle-a', 'cycle-b', 'Cycle A'),
        dispatch('cycle-b', 'cycle-a', 'Cycle B'),
      ]} />,
    )

    expect(screen.getByText('Orphan agent')).toBeVisible()
    expect(screen.getAllByText('Missing parent')).toHaveLength(1)
    expect(screen.getByText('Cycle A')).toBeVisible()
    expect(screen.getByText('Cycle B')).toBeVisible()
    expect(screen.getAllByText('Cycle detached')).toHaveLength(2)
  })

  it('renders only allowlisted artifact fields', () => {
    const item = dispatch('root', null, 'Agent one')
    item.artifact = {
      mode: 'handoff',
      outcome: 'accepted',
      reasoning: 'never render this',
      tool_io: 'private',
    } as AgentDispatchTrace['artifact']
    render(<DispatchDag dispatches={[item]} />)

    expect(screen.getByText('handoff')).toBeVisible()
    expect(screen.getByText('accepted')).toBeVisible()
    expect(screen.queryByText('never render this')).not.toBeInTheDocument()
    expect(screen.queryByText('private')).not.toBeInTheDocument()
  })
})
