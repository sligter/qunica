import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

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
    // Three signals, not one: the raised surface, the accent bar and the
    // weight. Colour alone would not survive a palette with two adjacent steps.
    expect(active).toHaveAttribute('aria-current', 'page')
    expect(active.className).toContain('bg-accent')
    expect(active.className).toContain('font-semibold')
    expect(active.className).toContain('before:bg-primary')
    expect(screen.getByRole('link', { name: /Reviewer/ })).not.toHaveAttribute('aria-current')
  })

  it('keeps one create action: the header while there is a list', () => {
    renderColumn()
    expect(screen.getAllByRole('link', { name: /new agent/i })).toHaveLength(1)
  })

  it('hands the create action to the empty state once the list is empty', () => {
    renderColumnWith({ items: [] })
    // The header button would sit a few pixels from the empty state's own, so
    // it stands down rather than offering the same thing twice.
    const links = screen.getAllByRole('link', { name: /new agent/i })
    expect(links).toHaveLength(1)
    fireEvent.click(links[0]!)
    expect(screen.getByText('at /agents/new')).toBeInTheDocument()
  })

  it('moves focus through the rows with the arrow keys', () => {
    renderColumn()
    const list = screen.getByRole('list', { name: 'Agents' })
    fireEvent.keyDown(list, { key: 'ArrowDown' })
    expect(document.activeElement).toBe(screen.getByRole('link', { name: /Reviewer/ }))
    fireEvent.keyDown(list, { key: 'ArrowDown' })
    expect(document.activeElement).toBe(screen.getByRole('link', { name: /Writer/ }))
    fireEvent.keyDown(list, { key: 'Home' })
    expect(document.activeElement).toBe(screen.getByRole('link', { name: /Reviewer/ }))
  })

  it('drops into the list from the search box', () => {
    renderColumn()
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Search agents' }), {
      key: 'ArrowDown',
    })
    expect(document.activeElement).toBe(screen.getByRole('link', { name: /Reviewer/ }))
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

  it('labels the header create link for assistive tech', () => {
    renderColumn()
    expect(screen.getByRole('link', { name: 'New agent' })).toHaveAttribute(
      'aria-label',
      'New agent',
    )
  })

  it('opens the row menu and renames from its dialog', async () => {
    const onRename = vi.fn().mockResolvedValue(undefined)
    renderColumnWith({ items, onRename, onDelete: vi.fn() })

    fireEvent.click(screen.getByRole('button', { name: 'Actions for Reviewer' }))
    expect(screen.getByRole('menu')).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Copy ID' })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const input = screen.getByRole('textbox', { name: 'Name' })
    fireEvent.change(input, { target: { value: 'Review bot' } })
    fireEvent.submit(input.closest('form')!)

    await waitFor(() => {
      expect(onRename).toHaveBeenCalledWith(items[0], 'Review bot')
    })
  })
})
