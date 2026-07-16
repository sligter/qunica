import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it } from 'vitest'

import { AgentActivityBubble } from '@/components/chat/AgentActivityBubble'

afterEach(cleanup)

describe('AgentActivityBubble', () => {
  it('collapses all reasoning and tools into one disclosure by default', async () => {
    const user = userEvent.setup()
    render(
      <AgentActivityBubble
        reasoning={[
          { id: 'reasoning-1', content: 'Inspect the workspace.' },
          { id: 'reasoning-2', content: 'Compare the relevant files.' },
        ]}
        tools={[
          {
            id: 'tool-1',
            name: 'Glob',
            status: 'completed',
            argsSummary: '*.tsx',
            resultSummary: '3 files',
          },
          { id: 'tool-2', name: 'Read', status: 'completed' },
        ]}
      />,
    )

    const activity = screen.getByRole('group', {
      name: 'Activity: 2 reasoning, 2 tools',
    }) as HTMLDetailsElement
    expect(activity.open).toBe(false)
    expect(within(activity).getByText('2 reasoning · 2 tools')).toBeInTheDocument()

    await user.click(within(activity).getByText('Activity'))
    expect(activity.open).toBe(true)
    expect(within(activity).getByText('Inspect the workspace.')).toBeVisible()
    expect(within(activity).getByText('Compare the relevant files.')).toBeVisible()
    expect(within(activity).getByText('Glob')).toBeVisible()
    expect(within(activity).getByText('Read')).toBeVisible()
  })

  it('keeps each tool inspectable inside the activity disclosure', async () => {
    const user = userEvent.setup()
    render(
      <AgentActivityBubble
        reasoning={[]}
        tools={[
          {
            id: 'tool-1',
            name: 'Read',
            status: 'completed',
            argsSummary: 'README.md',
            resultSummary: 'Project notes',
          },
        ]}
      />,
    )

    const activity = screen.getByRole('group', { name: 'Activity: 1 tool' }) as HTMLDetailsElement
    await user.click(within(activity).getByText('Activity'))
    const tool = within(activity).getByText('Read').closest('details') as HTMLDetailsElement
    expect(tool.open).toBe(false)

    await user.click(within(tool).getByText('Read'))
    expect(tool.open).toBe(true)
    await user.click(within(tool).getByText('Arguments'))
    await user.click(within(tool).getByText('Result'))
    expect(within(tool).getByText('README.md')).toBeVisible()
    expect(within(tool).getByText('Project notes')).toBeVisible()
  })

  it('reports active reasoning without opening automatically', () => {
    render(
      <AgentActivityBubble
        active
        reasoning={[{ id: 'reasoning-1', content: 'Still working', streaming: true }]}
        tools={[]}
      />,
    )

    const activity = screen.getByRole('group', {
      name: 'Activity: 1 reasoning, active',
    }) as HTMLDetailsElement
    expect(activity.open).toBe(false)
    expect(within(activity).getByText('1 reasoning')).toBeInTheDocument()
    expect(within(activity).queryByText(/tool/i)).not.toBeInTheDocument()
  })

  it('renders nothing when both activity categories are empty', () => {
    const { container } = render(<AgentActivityBubble reasoning={[]} tools={[]} />)
    expect(container).toBeEmptyDOMElement()
  })
})
