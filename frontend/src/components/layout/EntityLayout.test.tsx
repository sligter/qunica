import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { Link, MemoryRouter, Route, Routes, useParams } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import { EntityLayout } from '@/components/layout/EntityLayout'
import '@/i18n'

afterEach(cleanup)

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
            <Route index element={<p>Index</p>} />
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

  it('swaps list and detail panes below lg and exposes a return link', () => {
    render(
      <MemoryRouter initialEntries={['/agents/one']}>
        <Routes>
          <Route
            path="/agents"
            element={
              <EntityLayout
                titleKey="agents"
                list={<nav><Link to="/agents/one">One</Link></nav>}
              />
            }
          >
            <Route index element={<p>Index</p>} />
            <Route path=":id" element={<Draft />} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    expect(document.querySelector('[data-slot="entity-list-pane"]')).toHaveClass('max-lg:hidden')
    fireEvent.click(screen.getByRole('link', { name: 'Back to list' }))
    expect(screen.getByText('Index')).toBeInTheDocument()
    expect(document.querySelector('[data-slot="entity-detail-pane"]')).toHaveClass('max-lg:hidden')
  })
})
