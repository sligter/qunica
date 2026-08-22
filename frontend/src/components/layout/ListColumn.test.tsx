import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { ListColumn, type ListColumnItem } from '@/components/layout/ListColumn'

const items: ListColumnItem[] = [
  {
    id: 'a1',
    to: '/agents/a1',
    name: 'Reviewer',
    summary: 'Reviews patches',
    avatarClass: 'bg-avatar-1 text-avatar-foreground',
    avatarInitial: 'R',
  },
  {
    id: 'a2',
    to: '/agents/a2',
    name: 'Writer',
    summary: 'Drafts documents',
    avatarClass: 'bg-avatar-2 text-avatar-foreground',
    avatarInitial: 'W',
  },
]

/** The column is a pane, not a layout: navigation is asserted via the URL. */
function LocationProbe() {
  const { pathname } = useLocation()
  return <p>at {pathname}</p>
}

function renderColumn(initialEntry = '/agents') {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <div className="flex h-full">
        <ListColumn
          title="Agents"
          newTo="/agents/new"
          newLabel="New agent"
          searchPlaceholder="Search agents"
          isLoading={false}
          loadError={false}
          errorText="Failed to load."
          emptyText="No agents yet."
          items={items}
        />
        <LocationProbe />
      </div>
    </MemoryRouter>,
  )
}

function renderColumnWith(props: Partial<Parameters<typeof ListColumn>[0]>) {
  return render(
    <MemoryRouter initialEntries={['/agents']}>
      <div className="flex h-full">
        <ListColumn
          title="Agents"
          newTo="/agents/new"
          newLabel="New agent"
          searchPlaceholder="Search agents"
          isLoading={false}
          loadError={false}
          errorText="Failed to load."
          emptyText="No agents yet."
          items={[]}
          {...props}
        />
        <LocationProbe />
      </div>
    </MemoryRouter>,
  )
}

describe('ListColumn', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('renders rows with names and summaries', () => {
    renderColumn()
    expect(screen.getByText('Reviewer')).toBeInTheDocument()
    expect(screen.getByText('Writer')).toBeInTheDocument()
    expect(screen.getByText('Reviews patches')).toBeInTheDocument()
  })

  it('filters as you type and reports the match count', () => {
    renderColumn()
    fireEvent.change(screen.getByRole('textbox', { name: 'Search agents' }), {
      target: { value: 'wri' },
    })
    expect(screen.queryByText('Reviewer')).not.toBeInTheDocument()
    expect(screen.getByText('Writer')).toBeInTheDocument()
    expect(screen.getByText(/1 match/i)).toBeInTheDocument()
  })

  it('clears the query from the clear button', () => {
    renderColumn()
    const input = screen.getByRole('textbox', { name: 'Search agents' })
    fireEvent.change(input, { target: { value: 'wri' } })
    fireEvent.click(screen.getByRole('button', { name: /clear search/i }))
    expect(input).toHaveValue('')
    expect(screen.getByText('Reviewer')).toBeInTheDocument()
  })

  it('shows a no-matches state for queries that hit nothing', () => {
    renderColumn()
    fireEvent.change(screen.getByRole('textbox', { name: 'Search agents' }), {
      target: { value: 'zzz' },
    })
    // The message appears in the pane and again in the footer live region.
    expect(screen.getAllByText('No matches.').length).toBeGreaterThan(0)
  })

  it('shows the total count in the footer', () => {
    renderColumn()
    expect(screen.getByText(/2 items/i)).toBeInTheDocument()
  })

  it('highlights the row matching the active route', () => {
    renderColumn('/agents/a2')
    const active = screen.getByRole('link', { name: /Writer/ })
    expect(active.className).toContain('bg-primary/10')
  })

  it('renders skeleton rows while loading', () => {
    renderColumnWith({ isLoading: true })
    // Skeleton rows are aria-hidden placeholders; no real rows appear.
    expect(screen.queryByText('No agents yet.')).not.toBeInTheDocument()
    expect(document.querySelectorAll('.stream-skeleton').length).toBeGreaterThan(0)
  })

  it('renders an error state when loading failed', () => {
    renderColumnWith({ loadError: true })
    expect(screen.getByRole('alert')).toHaveTextContent('Failed to load.')
  })

  it('offers the create action in the empty state', () => {
    renderColumnWith({ items: [] })
    fireEvent.click(screen.getAllByRole('link', { name: /new agent/i })[0]!)
    expect(screen.getByText('at /agents/new')).toBeInTheDocument()
  })

  it('labels the header create link for assistive tech', () => {
    renderColumn()
    expect(screen.getByRole('link', { name: 'New agent' })).toHaveAttribute(
      'aria-label',
      'New agent',
    )
  })
})
