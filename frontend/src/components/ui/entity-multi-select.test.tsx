import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { EntityMultiSelect, type EntityMultiSelectItem } from '@/components/ui/entity-multi-select'
import i18n from '@/i18n'

function makeItems(count: number): EntityMultiSelectItem[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `agent-${index}`,
    name: `Agent ${String(index).padStart(3, '0')}`,
    description: index % 2 === 0 ? 'Reviews pull requests' : 'Writes release notes',
  }))
}

function Harness({
  items,
  initial = [],
  searchThreshold,
}: {
  items: EntityMultiSelectItem[]
  initial?: string[]
  searchThreshold?: number
}) {
  const [selectedIds, setSelectedIds] = useState(initial)
  return (
    <>
      <EntityMultiSelect
        items={items}
        selectedIds={selectedIds}
        onChange={setSelectedIds}
        label="Agent as tool"
        searchPlaceholder="Search agents"
        emptyText="No other agents are available."
        namePrefix="@"
        searchThreshold={searchThreshold}
      />
      <p data-testid="selection">{selectedIds.join(',')}</p>
    </>
  )
}

function listbox() {
  return screen.getByRole('listbox', { name: 'Agent as tool' })
}

describe('EntityMultiSelect', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('renders a bounded page of a large catalog instead of every option', () => {
    render(<Harness items={makeItems(120)} />)

    expect(within(listbox()).getAllByRole('option')).toHaveLength(40)
    expect(screen.getByRole('status')).toHaveTextContent('Showing 40 of 120')
    expect(screen.getByRole('button', { name: 'All (120)' })).toBeInTheDocument()
  })

  it('extends the rendered page on demand', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} />)

    await user.click(screen.getByRole('button', { name: 'Show more' }))
    expect(within(listbox()).getAllByRole('option')).toHaveLength(80)
    expect(screen.getByRole('status')).toHaveTextContent('Showing 80 of 120')
  })

  it('narrows the catalog with a multi-token search and highlights the match', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} />)

    await user.type(screen.getByRole('combobox', { name: 'Search agents' }), 'agent 011')
    const options = within(listbox()).getAllByRole('option')
    expect(options).toHaveLength(1)
    expect(options[0]).toHaveTextContent('@Agent 011')
    expect(within(options[0]).getAllByText(/agent|011/i).length).toBeGreaterThan(0)
  })

  it('searches the description as well as the name', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(10)} />)

    await user.type(screen.getByRole('combobox', { name: 'Search agents' }), 'release')
    expect(within(listbox()).getAllByRole('option')).toHaveLength(5)
  })

  it('selects the active option with the arrow keys and Enter', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} />)

    const search = screen.getByRole('combobox', { name: 'Search agents' })
    await user.click(search)

    // Arrowing up before entering the list must not jump to the far tail.
    await user.keyboard('{ArrowUp}{Enter}')
    expect(screen.getByTestId('selection')).toHaveTextContent('')
    expect(within(listbox()).getAllByRole('option')).toHaveLength(40)

    await user.keyboard('{ArrowDown}{ArrowDown}{ArrowUp}{ArrowDown}{Enter}')
    expect(screen.getByTestId('selection')).toHaveTextContent('agent-1')
    expect(within(listbox()).getByRole('option', { name: /Agent 001/ })).toHaveAttribute(
      'aria-selected',
      'true',
    )
  })

  it('pages the window in as the arrow keys walk past the rendered rows', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} />)

    await user.click(screen.getByRole('combobox', { name: 'Search agents' }))
    await user.keyboard('{ArrowDown>41/}')

    expect(within(listbox()).getAllByRole('option')).toHaveLength(80)
    expect(screen.getByRole('status')).toHaveTextContent('Showing 80 of 120')
  })

  it('keeps selections visible as chips when the search filters them out', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} initial={['agent-7']} />)

    await user.type(screen.getByRole('combobox', { name: 'Search agents' }), 'Agent 100')
    expect(within(listbox()).getAllByRole('option')).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Remove Agent 007' })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Remove Agent 007' }))
    expect(screen.getByTestId('selection')).toHaveTextContent('')
  })

  it('reviews and clears the current selection through the Selected filter', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} initial={['agent-3', 'agent-9']} />)

    await user.click(screen.getByRole('button', { name: 'Selected (2)' }))
    const options = within(listbox()).getAllByRole('option')
    expect(options).toHaveLength(2)
    expect(options.every((option) => option.getAttribute('aria-selected') === 'true')).toBe(true)

    await user.click(screen.getByRole('button', { name: 'Clear all' }))
    expect(screen.getByTestId('selection')).toHaveTextContent('')
    expect(screen.getByRole('button', { name: 'Selected (0)' })).toBeDisabled()
  })

  it('bulk-selects the narrowed matches only while a search is active', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(120)} />)

    expect(screen.queryByRole('button', { name: /Select all/ })).not.toBeInTheDocument()

    // "Agent 10" matches 010, 100-109, and 110.
    await user.type(screen.getByRole('combobox', { name: 'Search agents' }), 'Agent 10')
    expect(within(listbox()).getAllByRole('option')).toHaveLength(12)

    await user.click(screen.getByRole('button', { name: 'Select all 12 matches' }))
    expect(screen.getByRole('button', { name: 'Selected (12)' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Select all/ })).not.toBeInTheDocument()
  })

  it('collapses an oversized chip row behind an expand toggle', async () => {
    const user = userEvent.setup()
    render(
      <Harness
        items={makeItems(120)}
        initial={Array.from({ length: 15 }, (_, index) => `agent-${index}`)}
      />,
    )

    expect(screen.getAllByRole('button', { name: /^Remove / })).toHaveLength(12)
    await user.click(screen.getByRole('button', { name: '+3 more' }))
    expect(screen.getAllByRole('button', { name: /^Remove / })).toHaveLength(15)
    expect(screen.queryByRole('button', { name: /\+\d+ more/ })).not.toBeInTheDocument()
  })

  it('surfaces a removable chip for a selection that no longer exists', async () => {
    const user = userEvent.setup()
    render(<Harness items={makeItems(12)} initial={['deleted-agent-id']} />)

    expect(screen.getByText('Unknown (deleted-…)')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Remove deleted-agent-id' }))
    expect(screen.getByTestId('selection')).toHaveTextContent('')
  })

  it('drops the search affordances for a small catalog and keeps rows tabbable', () => {
    render(<Harness items={makeItems(4)} initial={['agent-1']} />)

    expect(screen.queryByRole('combobox')).not.toBeInTheDocument()
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /^(All|Selected) \(/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /^Remove / })).not.toBeInTheDocument()
    expect(within(listbox()).getAllByRole('option')).toHaveLength(4)
    expect(within(listbox()).getAllByRole('option')[0]).toHaveAttribute('tabindex', '0')
  })

  it('still offers a way out of a stale selection in a small catalog', () => {
    render(<Harness items={makeItems(4)} initial={['deleted-agent-id']} />)

    expect(screen.queryByRole('combobox')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Remove deleted-agent-id' })).toBeInTheDocument()
  })

  it('reports an empty catalog with the caller copy', () => {
    render(<Harness items={[]} />)

    expect(screen.getByText('No other agents are available.')).toBeInTheDocument()
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()
  })

  it('translates its chrome without changing the selection', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<Harness items={makeItems(120)} initial={['agent-2']} />)

    expect(screen.getByRole('button', { name: '全部（120）' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '已选（1）' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '全部清除' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('已显示 40 / 120')
    expect(screen.getByTestId('selection')).toHaveTextContent('agent-2')
  })
})
