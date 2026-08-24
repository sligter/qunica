import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { useState } from 'react'
import { MemoryRouter, Route, Routes, useParams } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

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

function renderLayout(initialEntry: string) {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Routes>
        <Route path="/agents" element={<EntityLayout titleKey="agents" />}>
          <Route index element={<p>Index</p>} />
          <Route path="new" element={<p>Create</p>} />
          <Route path=":id" element={<Draft />} />
        </Route>
      </Routes>
    </MemoryRouter>,
  )
}

/** The resource rail also renders a link named after each area; every crumb
 *  assertion scopes to the layout's own heading to stay unambiguous. */
function headerCrumb() {
  const heading = screen.getByRole('heading', { level: 1 })
  return within(heading).getByRole('link', { name: 'Agents' })
}

describe('EntityLayout', () => {
  it('isolates editor state when switching entities', () => {
    renderLayout('/agents/one')

    fireEvent.change(screen.getByRole('textbox', { name: 'Draft' }), {
      target: { value: 'unsaved one' },
    })

    // Navigating home and back remounts the Outlet (key={pathname}), so the
    // draft resets to the new entity's id.
    fireEvent.click(headerCrumb())
    expect(screen.getByText('Index')).toBeInTheDocument()
    cleanup()

    render(
      <MemoryRouter initialEntries={['/agents/one', '/agents/two']}>
        <Routes>
          <Route path="/agents" element={<EntityLayout titleKey="agents" />}>
            <Route index element={<p>Index</p>} />
            <Route path=":id" element={<Draft />} />
          </Route>
        </Routes>
        {/* history.push via the router is not needed — key={pathname} on the
            Outlet already remounts per route. */}
      </MemoryRouter>,
    )
    expect(screen.getByRole('textbox', { name: 'Draft' })).toHaveValue('two')
  })

  it('links the area name back to the index when a detail page is open', () => {
    renderLayout('/agents/one')

    const crumb = headerCrumb()
    expect(crumb).toHaveAttribute('href', '/agents')

    fireEvent.click(crumb)
    expect(screen.getByText('Index')).toBeInTheDocument()
  })

  it('keeps the create route reachable and returns through the same crumb', () => {
    renderLayout('/agents/new')

    expect(screen.getByText('Create')).toBeInTheDocument()
    fireEvent.click(headerCrumb())
    expect(screen.getByText('Index')).toBeInTheDocument()
  })

  it('renders the area name without a separator on the index itself', () => {
    renderLayout('/agents')

    // Still a link home (harmless no-op navigation), but no chevron because
    // there is nothing deeper to point at.
    expect(headerCrumb()).toHaveAttribute('href', '/agents')
    const heading = screen.getByRole('heading', { level: 1 })
    expect(within(heading).queryByText('›')).toBeNull()
    expect(screen.getByText('Index')).toBeInTheDocument()
  })

  it('closes the native window from an auxiliary desktop surface', () => {
    vi.stubGlobal('__TAURI_INTERNALS__', {
      metadata: { currentWindow: { label: 'library' } },
    })
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...window.location, hostname: 'tauri.localhost' },
    })
    renderLayout('/agents')
    expect(screen.getByRole('button', { name: 'Close window' })).toBeInTheDocument()
    vi.unstubAllGlobals()
  })
})
