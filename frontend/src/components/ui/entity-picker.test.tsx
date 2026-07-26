import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import { EntityPicker, type PickerItem } from '@/components/ui/entity-picker'
import i18n from '@/i18n'

function items(count: number, prefix = 'item'): PickerItem[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `${prefix}-${index}`,
    label: `${prefix}-${String(index).padStart(3, '0')}`,
    meta: `description for ${prefix} ${index}`,
  }))
}

function picker() {
  return within(screen.getByRole('group', { name: 'Things' }))
}

beforeAll(async () => {
  await i18n.changeLanguage('en-US')
})

afterEach(cleanup)

describe('EntityPicker sizing', () => {
  it('renders a bare grid with no chrome for a short list', () => {
    // A search box in front of three items is chrome the user reads past to
    // reach a list they can already see whole.
    render(
      <EntityPicker label="Things" items={items(3)} selectedIds={[]} onChange={vi.fn()} />,
    )

    expect(picker().getAllByRole('checkbox')).toHaveLength(3)
    expect(picker().queryByPlaceholderText('Search')).toBeNull()
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('grows search, a chip tray and a scroller once the list stops fitting', () => {
    render(
      <EntityPicker
        label="Things"
        items={items(30)}
        selectedIds={['item-1']}
        onChange={vi.fn()}
      />,
    )

    expect(screen.getByRole('listbox', { name: 'Things' })).toBeTruthy()
    expect(picker().getByPlaceholderText('Search')).toBeTruthy()
    // The count is what tells the user the list is longer than the window.
    expect(screen.getByText('30 items · 1 selected')).toBeTruthy()
  })

  it('caps the scroller height so the surrounding form does not grow with the list', () => {
    const { rerender } = render(
      <EntityPicker label="Things" items={items(30)} selectedIds={[]} onChange={vi.fn()} />,
    )
    const at30 = screen.getByRole('listbox').style.maxHeight

    rerender(
      <EntityPicker label="Things" items={items(400)} selectedIds={[]} onChange={vi.fn()} />,
    )
    const at400 = screen.getByRole('listbox').style.maxHeight

    expect(at30).toBe(at400)
    expect(at30).toBe('256px')
  })

  it('renders every item, so nothing is unreachable behind a silent cap', () => {
    // A render cap with no affordance would leave the tail of a long list
    // invisible and unselectable, which reads as "I do not own those".
    render(
      <EntityPicker label="Things" items={items(400)} selectedIds={[]} onChange={vi.fn()} />,
    )

    expect(picker().getAllByRole('checkbox')).toHaveLength(400)
    expect(screen.getByText('item-399')).toBeTruthy()
  })

  it('shows the empty slot instead of an empty box when there is nothing to pick', () => {
    render(
      <EntityPicker
        label="Things"
        items={[]}
        selectedIds={[]}
        onChange={vi.fn()}
        empty={<p>Nothing configured yet</p>}
      />,
    )

    expect(screen.getByText('Nothing configured yet')).toBeTruthy()
  })
})

describe('EntityPicker selection', () => {
  it('adds and removes ids without reordering the list', async () => {
    const onChange = vi.fn()
    render(
      <EntityPicker
        label="Things"
        items={items(30)}
        selectedIds={['item-2']}
        onChange={onChange}
      />,
    )

    const boxes = picker().getAllByRole('checkbox')
    await userEvent.click(boxes[0]!)
    expect(onChange).toHaveBeenCalledWith(['item-2', 'item-0'])

    // The list order is alphabetical and does not move selected rows to the
    // top; re-sorting under the cursor turns multi-select into a chase.
    const labels = picker()
      .getAllByRole('checkbox')
      .map((box) => box.closest('[data-picker-row]')?.textContent ?? '')
    expect(labels[0]).toContain('item-000')
    expect(labels[2]).toContain('item-002')
  })

  it('filters on label, description and hidden keywords', async () => {
    render(
      <EntityPicker
        label="Things"
        items={[
          { id: 'a', label: 'alpha', meta: 'the first letter' },
          { id: 'b', label: 'beta', meta: 'the second letter' },
          ...items(10, 'filler'),
          { id: 'c', label: 'gamma', keywords: 'mcp__weather__forecast' },
        ]}
        selectedIds={[]}
        onChange={vi.fn()}
      />,
    )

    const search = picker().getByPlaceholderText('Search')
    await userEvent.type(search, 'second')
    await waitFor(() => expect(picker().getAllByRole('checkbox')).toHaveLength(1))
    expect(screen.getByText('beta')).toBeTruthy()

    await userEvent.clear(search)
    // A keyword matches even though it is never rendered.
    await userEvent.type(search, 'weather')
    await waitFor(() => expect(picker().getAllByRole('checkbox')).toHaveLength(1))
    expect(screen.getByText('gamma')).toBeTruthy()
  })

  it('keeps a selected item reachable at length through the selected-only filter', async () => {
    // Scrolling 400 rows to un-tick one is the failure mode this replaces.
    render(
      <EntityPicker
        label="Things"
        items={items(400)}
        selectedIds={['item-350']}
        onChange={vi.fn()}
      />,
    )

    await userEvent.click(screen.getByRole('button', { name: '1 selected' }))

    await waitFor(() => expect(picker().getAllByRole('checkbox')).toHaveLength(1))
    expect(screen.getByText('item-350')).toBeTruthy()
  })

  it('removes a selection from the chip tray without scrolling the list', async () => {
    const onChange = vi.fn()
    render(
      <EntityPicker
        label="Things"
        items={items(30)}
        selectedIds={['item-20']}
        onChange={onChange}
      />,
    )

    await userEvent.click(screen.getByRole('button', { name: 'Remove item-020' }))

    expect(onChange).toHaveBeenCalledWith([])
  })

  it('clears every selection at once', async () => {
    const onChange = vi.fn()
    render(
      <EntityPicker
        label="Things"
        items={items(30)}
        selectedIds={['item-1', 'item-2']}
        onChange={onChange}
      />,
    )

    await userEvent.click(screen.getByRole('button', { name: 'Clear' }))

    expect(onChange).toHaveBeenCalledWith([])
  })

  it('refuses to toggle a row that says why it is unavailable', async () => {
    const onChange = vi.fn()
    render(
      <EntityPicker
        label="Things"
        items={[
          ...items(10),
          { id: 'blocked', label: 'blocked', disabledReason: 'Needs a sandbox' },
        ]}
        selectedIds={[]}
        onChange={onChange}
      />,
    )

    const blocked = screen.getByText('blocked').closest('[data-picker-row]')!
    const box = within(blocked as HTMLElement).getByRole('checkbox')
    expect(box).toBeDisabled()
    await userEvent.click(box)

    expect(onChange).not.toHaveBeenCalled()
    // The reason is wired to the control rather than stacked as another line.
    expect(box.getAttribute('aria-describedby')).toBeTruthy()
    expect(screen.getByText('Needs a sandbox')).toBeTruthy()
  })

  it('reports when a search matches nothing rather than showing a blank box', async () => {
    render(
      <EntityPicker label="Things" items={items(30)} selectedIds={[]} onChange={vi.fn()} />,
    )

    await userEvent.type(picker().getByPlaceholderText('Search'), 'zzzznope')

    await waitFor(() => expect(screen.getByText('No matches.')).toBeTruthy())
  })
})
