import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { Link, MemoryRouter, Route, Routes, useParams } from 'react-router-dom'
import { describe, expect, it } from 'vitest'

import { EntityLayout } from '@/components/layout/EntityLayout'

function Draft() {
  const { id } = useParams<{ id: string }>()
  const [value, setValue] = useState(id)
  return (
    <input
      aria-label="Draft"
      value={value}
      onChange={(event) => setValue(event.target.value)}
    />
  )
}

describe('EntityLayout', () => {
  it('isolates editor state when switching entities', () => {
    render(
      <MemoryRouter initialEntries={['/agents/one']}>
        <Routes>
          <Route
            path="/agents"
            element={
              <EntityLayout
                titleKey="agents"
                list={
                  <nav>
                    <Link to="/agents/one">One</Link>
                    <Link to="/agents/two">Two</Link>
                  </nav>
                }
              />
            }
          >
            <Route path=":id" element={<Draft />} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    fireEvent.change(screen.getByRole('textbox', { name: 'Draft' }), {
      target: { value: 'unsaved one' },
    })
    fireEvent.click(screen.getByRole('link', { name: 'Two' }))

    expect(screen.getByRole('textbox', { name: 'Draft' })).toHaveValue('two')
  })
})
