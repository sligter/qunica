import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Composer } from '@/components/chat/Composer'
import type { GroupAgentRead } from '@/types/api'

vi.mock('@/hooks/useGroupFiles', () => ({
  WorkspaceUploadManyError: class WorkspaceUploadManyError extends Error {},
  useUploadGroupWorkspaceFiles: () => ({
    isPending: false,
    mutateAsync: vi.fn(),
  }),
}))

const groupAgents: GroupAgentRead[] = [
  {
    id: 'group-agent-1',
    group_id: 'group-1',
    agent_id: 'agent-1',
    display_name: 'Planner',
    role: null,
    topology_role: null,
    speaking_order: null,
    response_mode: 'default',
    share_group_workspace: false,
    context_usage: null,
    status: 'active',
    joined_at: '2026-07-18T00:00:00Z',
  },
]

describe('Composer mentions', () => {
  afterEach(cleanup)

  it.each([
    ['Tab', '{Tab}'],
    ['Enter', '{Enter}'],
    ['Space', ' '],
  ])('selects a filtered mention with %s', async (_label, key) => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    render(<Composer onSend={onSend} groupAgents={groupAgents} />)
    const textarea = screen.getByRole('textbox', { name: 'Message' })

    await user.type(textarea, '@pla')
    await user.keyboard(key)

    expect(textarea).toHaveValue('@Planner ')
    expect(onSend).not.toHaveBeenCalled()
  })

  it('summarizes large groups and reveals remaining agents on demand', async () => {
    const user = userEvent.setup()
    const agents = ['Planner', 'Researcher', 'Writer', 'Reviewer', 'Operator'].map(
      (display_name, index) => ({
        ...groupAgents[0],
        id: `group-agent-${index + 1}`,
        agent_id: `agent-${index + 1}`,
        display_name,
      }),
    )

    render(<Composer onSend={vi.fn()} groupAgents={agents} />)

    expect(screen.getByText('@Planner')).toBeVisible()
    expect(screen.getByText('@Researcher')).toBeVisible()
    expect(screen.getByText('@Writer')).toBeVisible()
    expect(screen.queryByText('@Reviewer')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Show 2 more agents' }))
    expect(screen.getByText('@Reviewer')).toBeVisible()
    expect(screen.getByText('@Operator')).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Close agent list' }))
    expect(screen.queryByText('@Reviewer')).not.toBeInTheDocument()
  })
})
