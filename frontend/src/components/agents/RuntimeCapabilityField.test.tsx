import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { RuntimeCapabilityField } from '@/components/agents/RuntimeCapabilityField'

const options = [
  { value: 'gpt-live', label: 'GPT Live', description: 'Discovered at runtime' },
  { value: 'gpt-fast', label: 'GPT Fast' },
]

function EditableField({
  onCommit = vi.fn(),
  onRefresh,
  isLoading = false,
  stale = false,
  warning = null,
}: {
  onCommit?: (value: string) => void
  onRefresh?: () => void
  isLoading?: boolean
  stale?: boolean
  warning?: string | null
}) {
  const [value, setValue] = useState('saved-custom')
  return (
    <RuntimeCapabilityField
      id="runtime-model"
      label="Model"
      value={value}
      options={options}
      placeholder="Provider default"
      onChange={setValue}
      onCommit={onCommit}
      onRefresh={onRefresh}
      isLoading={isLoading}
      stale={stale}
      warning={warning}
    />
  )
}

function ProgrammaticValueField({ onCommit }: { onCommit: (value: string) => void }) {
  const [value, setValue] = useState('saved-custom')
  return (
    <>
      <button type="button" onClick={() => setValue('preset-model')}>
        Apply preset
      </button>
      <RuntimeCapabilityField
        id="programmatic-model"
        label="Model"
        value={value}
        options={options}
        placeholder="Adapter default"
        onChange={setValue}
        onCommit={onCommit}
      />
    </>
  )
}

describe('RuntimeCapabilityField', () => {
  afterEach(cleanup)

  it('keeps arbitrary values editable and exposes suggestions through a stable datalist', async () => {
    const user = userEvent.setup()
    const onCommit = vi.fn()
    const view = render(<EditableField onCommit={onCommit} />)
    const input = screen.getByRole('combobox', { name: 'Model' })

    expect(input).toHaveValue('saved-custom')
    const listId = input.getAttribute('list')
    expect(listId).toBe('runtime-model-available-values')
    expect(view.container.querySelector(`#${listId} option[value="gpt-live"]`)).toHaveAttribute(
      'label',
      'GPT Live',
    )

    await user.clear(input)
    await user.type(input, 'my-private-model')
    expect(input).toHaveValue('my-private-model')
    expect(onCommit).not.toHaveBeenCalled()

    await user.tab()
    expect(onCommit).toHaveBeenCalledOnce()
    expect(onCommit).toHaveBeenCalledWith('my-private-model')
  })

  it('commits an exact suggestion without probing on each partial keystroke', () => {
    const onCommit = vi.fn()
    render(<EditableField onCommit={onCommit} />)
    const input = screen.getByRole('combobox', { name: 'Model' })

    fireEvent.change(input, { target: { value: 'gpt-' } })
    expect(onCommit).not.toHaveBeenCalled()

    fireEvent.change(input, { target: { value: 'gpt-live' } })
    expect(onCommit).toHaveBeenCalledOnce()
    expect(onCommit).toHaveBeenCalledWith('gpt-live')
  })

  it('commits a return to the initial value after a programmatic preset change', async () => {
    const user = userEvent.setup()
    const onCommit = vi.fn()
    render(<ProgrammaticValueField onCommit={onCommit} />)

    await user.click(screen.getByRole('button', { name: 'Apply preset' }))
    const input = screen.getByRole('combobox', { name: 'Model' })
    expect(input).toHaveValue('preset-model')

    await user.clear(input)
    await user.type(input, 'saved-custom')
    await user.tab()

    expect(onCommit).toHaveBeenCalledOnce()
    expect(onCommit).toHaveBeenCalledWith('saved-custom')
  })

  it('provides accessible refresh, loading, warning, and stale states', async () => {
    const user = userEvent.setup()
    const onRefresh = vi.fn()
    const view = render(
      <EditableField onRefresh={onRefresh} warning="Adapter could not list models." />,
    )

    expect(screen.getByRole('status')).toHaveTextContent('Adapter could not list models.')
    await user.click(screen.getByRole('button', { name: 'Refresh available values' }))
    expect(onRefresh).toHaveBeenCalledOnce()

    view.rerender(<EditableField onRefresh={onRefresh} stale />)
    expect(screen.getByRole('status')).toHaveTextContent(
      'Runtime settings changed. Refresh available values.',
    )

    view.rerender(<EditableField onRefresh={onRefresh} isLoading />)
    expect(screen.getByRole('status')).toHaveTextContent('Loading available values...')
    expect(screen.getByRole('button', { name: 'Refresh available values' })).toBeDisabled()
  })
})
