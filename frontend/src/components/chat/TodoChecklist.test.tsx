import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { TodoChecklist } from '@/components/chat/TodoChecklist'
import i18n from '@/i18n'
import type { TodoItem } from '@/types/api'

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

const todos: TodoItem[] = [
  { content: 'read the code', status: 'completed' },
  { content: 'write the fix', status: 'in_progress' },
  { content: 'run the tests', status: 'pending' },
]

describe('TodoChecklist', () => {
  it('shows every item with the status the agent recorded', () => {
    render(<TodoChecklist todos={todos} />)

    for (const todo of todos) {
      expect(screen.getByText(todo.content)).toBeVisible()
    }
    // The status is what makes this a checklist rather than a list, so it has
    // to reach assistive tech and not only the icon shape.
    expect(screen.getByLabelText('In progress')).toBeVisible()
    expect(screen.getByLabelText('Done')).toBeVisible()
    expect(screen.getByLabelText('To do')).toBeVisible()
  })

  it('reports how far along the work is', () => {
    render(<TodoChecklist todos={todos} />)

    expect(screen.getByText('1/3 done')).toBeVisible()
  })

  it('renders nothing when the agent kept no checklist', () => {
    const { container } = render(<TodoChecklist todos={[]} />)

    expect(container).toBeEmptyDOMElement()
  })
})
