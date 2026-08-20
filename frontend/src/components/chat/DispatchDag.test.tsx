import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { DispatchDag } from '@/components/chat/DispatchDag'
import i18n from '@/i18n'
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
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
  })

  it('builds parent-child adjacency in stable dispatch order', () => {
    render(<DispatchDag dispatches={[
      dispatch('root', null),
      dispatch('child-a', 'root'),
      dispatch('child-b', 'root'),
    ]} />)
    const rows = screen.getByRole('list', { name: 'Dispatch path' }).querySelectorAll(':scope > li')
    expect(Array.from(rows).map((row) => row.getAttribute('data-dispatch-id'))).toEqual([
      'root',
      'child-a',
      'child-b',
    ])
    expect(rows[1]).toHaveAttribute('data-visual-depth', '1')
    expect(rows[2]).toHaveAttribute('data-visual-depth', '1')
  })

  it('renders agent names while retaining IDs as diagnostics', () => {
    render(
      <DispatchDag
        dispatches={[dispatch('dispatch-1', null, 'agent-1')]}
        agentName={(id) => id === 'agent-1' ? 'Researcher' : id}
      />,
    )

    const speaker = screen.getByText('Researcher')
    expect(speaker).toBeVisible()
    expect(speaker).toHaveAttribute('title', 'agent-1')
    expect(screen.queryByText('agent-1')).not.toBeInTheDocument()
  })

  it('renders orphan and cycle records once without recursing forever', () => {
    render(
      <DispatchDag dispatches={[
        dispatch('orphan', 'missing', 'Orphan agent'),
        dispatch('cycle-a', 'cycle-b', 'Cycle A'),
        dispatch('cycle-b', 'cycle-a', 'Cycle B'),
        dispatch('cycle-child', 'cycle-b', 'Cycle child'),
      ]} />,
    )

    expect(screen.getByText('Orphan agent')).toBeVisible()
    expect(screen.getAllByText('Missing parent')).toHaveLength(1)
    expect(screen.getByText('Cycle A')).toBeVisible()
    expect(screen.getByText('Cycle B')).toBeVisible()
    expect(screen.getAllByText('Cycle detached')).toHaveLength(1)
    expect(screen.getByText('Cycle child')).toBeVisible()
    expect(
      screen.getByText('Cycle child').closest('li')?.querySelector('[data-edge-issue]'),
    ).toBeNull()
  })

  it('caps visual indentation without changing deep adjacency order', () => {
    const chain = Array.from({ length: 10 }, (_, index) =>
      dispatch(`node-${index}`, index === 0 ? null : `node-${index - 1}`),
    )
    render(<DispatchDag dispatches={chain} />)

    const rows = screen.getByRole('list', { name: 'Dispatch path' }).querySelectorAll(':scope > li')
    expect(rows).toHaveLength(10)
    expect(Array.from(rows).map((row) => row.getAttribute('data-dispatch-id'))).toEqual(
      chain.map((item) => item.id),
    )
    expect(rows[9]).toHaveAttribute('data-visual-depth', '3')
    expect(rows[9]).toHaveStyle({ paddingInlineStart: '36px' })
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

    expect(screen.getByText('Handoff')).toBeVisible()
    expect(screen.getByText('accepted')).toBeVisible()
    expect(screen.queryByText('never render this')).not.toBeInTheDocument()
    expect(screen.queryByText('private')).not.toBeInTheDocument()
  })

  it('localizes known dispatch framing while preserving raw agent and artifact values', async () => {
    const item = dispatch('root', null, 'Agent_RAW_42')
    item.selection_reason = 'deterministic_order'
    item.action_kind = 'handoff'
    item.status = 'waiting_for_user'
    item.hop = 2
    item.total_tokens = 12345
    item.artifact = {
      mode: 'handoff',
      target_agent_id: 'target/raw-id',
      child_dispatch_id: 'child_RAW',
      outcome: 'SERVER_OUTCOME_RAW',
      failure_code: 'FAILURE_CODE_RAW',
    }

    await i18n.changeLanguage('zh-CN')
    render(<DispatchDag dispatches={[item]} />)

    expect(screen.getByRole('list', { name: '调度路径' })).toBeVisible()
    expect(screen.getAllByText('转交')).toHaveLength(2)
    expect(screen.getByText('等待用户输入')).toBeVisible()
    expect(screen.getByText('确定性顺序')).toBeVisible()
    expect(screen.getByText('第 2 跳')).toBeVisible()
    expect(screen.getByText('12,345 Token')).toBeVisible()
    expect(screen.getByText('Agent_RAW_42')).toBeVisible()
    expect(screen.getByText('target/raw-id')).toBeVisible()
    expect(screen.getByText('child_RAW')).toBeVisible()
    expect(screen.getByText('SERVER_OUTCOME_RAW')).toBeVisible()
    expect(screen.getByText('FAILURE_CODE_RAW')).toBeVisible()
  })

  it('frames unknown wire dispatch values without passing missing keys to i18n', async () => {
    const item = dispatch('root', null, 'Agent_RAW')
    item.action_kind = 'future_action' as AgentDispatchTrace['action_kind']
    item.status = 'future_status' as AgentDispatchTrace['status']
    item.selection_reason = 'future_reason' as AgentDispatchTrace['selection_reason']
    item.artifact = {
      mode: 'future_mode',
      outcome: 'OUTCOME_RAW',
    } as unknown as AgentDispatchTrace['artifact']

    await i18n.changeLanguage('zh-CN')
    render(<DispatchDag dispatches={[item]} />)

    expect(screen.getByText('操作：future_action')).toBeVisible()
    expect(screen.getByText('状态：future_status')).toBeVisible()
    expect(screen.getByText('原因：future_reason')).toBeVisible()
    expect(screen.getByText('模式')).toBeVisible()
    expect(screen.getByText('future_mode')).toBeVisible()
    expect(screen.getByText('OUTCOME_RAW')).toBeVisible()
  })
})
