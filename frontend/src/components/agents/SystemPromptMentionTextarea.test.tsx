import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { SystemPromptMentionTextarea } from '@/components/agents/SystemPromptMentionTextarea'
import '@/i18n'

vi.mock('@/hooks/useAgents', () => ({
  useAgents: () => ({
    data: [
      {
        id: 'agent-1',
        name: 'Planner',
        description: 'Plans implementation work',
      },
    ],
  }),
}))

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({ data: [] }),
}))

function EditableMentionTextarea() {
  const [value, setValue] = useState('')

  return (
    <SystemPromptMentionTextarea
      aria-label="System prompt"
      value={value}
      onChange={setValue}
    />
  )
}

describe('SystemPromptMentionTextarea', () => {
  afterEach(cleanup)

  it('selects the highlighted mention with Tab', async () => {
    const user = userEvent.setup()
    render(<EditableMentionTextarea />)
    const textarea = screen.getByRole('textbox', { name: 'System prompt' })

    await user.type(textarea, '@')
    expect(screen.getByRole('listbox')).toBeInTheDocument()

    await user.keyboard('{Tab}')

    expect(textarea).toHaveValue('[Agent: Planner]\nDescription: Plans implementation work')
  })

  it('selects the highlighted mention with Space while suggestions are visible', async () => {
    const user = userEvent.setup()
    render(<EditableMentionTextarea />)
    const textarea = screen.getByRole('textbox', { name: 'System prompt' })

    await user.type(textarea, '@')
    await user.keyboard(' ')

    expect(textarea).toHaveValue('[Agent: Planner]\nDescription: Plans implementation work')
  })

  it('allows Space as regular text when no suggestions are visible', async () => {
    const user = userEvent.setup()
    render(<EditableMentionTextarea />)
    const textarea = screen.getByRole('textbox', { name: 'System prompt' })

    await user.type(textarea, 'hello world')

    expect(textarea).toHaveValue('hello world')
  })
})
