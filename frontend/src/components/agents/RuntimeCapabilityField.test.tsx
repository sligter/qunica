import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { RuntimeCapabilityField } from '@/components/agents/RuntimeCapabilityField'
import i18n from '@/i18n'

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
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('keeps arbitrary values editable and exposes a styled suggestion list', async () => {
    const user = userEvent.setup()
    const onCommit = vi.fn()
    render(<EditableField onCommit={onCommit} />)
    const input = screen.getByRole('combobox', { name: 'Model' })

    expect(input).toHaveValue('saved-custom')
    await user.click(input)
    expect(screen.getByRole('listbox', { name: 'Model options' })).toHaveClass('bg-card')
    expect(screen.getByRole('option', { name: /GPT Live/i })).toHaveTextContent(
      'Discovered at runtime',
    )

    await user.clear(input)
    await user.type(input, 'my-private-model')
    expect(input).toHaveValue('my-private-model')
    expect(onCommit).not.toHaveBeenCalled()

    await user.tab()
    expect(onCommit).toHaveBeenCalledOnce()
    expect(onCommit).toHaveBeenCalledWith('my-private-model')
  })

  it('selects a filtered option through click and keyboard navigation', async () => {
    const user = userEvent.setup()
    const onCommit = vi.fn()
    render(<EditableField onCommit={onCommit} />)
    const input = screen.getByRole('combobox', { name: 'Model' })

    await user.click(input)
    await user.clear(input)
    await user.type(input, 'fast')
    await user.click(screen.getByRole('option', { name: /GPT Fast/i }))
    expect(input).toHaveValue('gpt-fast')
    expect(onCommit).toHaveBeenCalledWith('gpt-fast')

    await user.click(input)
    await user.clear(input)
    await user.keyboard('{ArrowDown}{Enter}')
    expect(input).toHaveValue('gpt-live')
  })

  it('closes the suggestion list when the user clicks outside the field', async () => {
    const user = userEvent.setup()
    render(
      <>
        <EditableField />
        <button type="button">Other control</button>
      </>,
    )

    await user.click(screen.getByRole('combobox', { name: 'Model' }))
    expect(screen.getByRole('listbox', { name: 'Model options' })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Other control' }))
    expect(screen.queryByRole('listbox', { name: 'Model options' })).not.toBeInTheDocument()
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

  it('translates capability actions and state framing without changing runtime values', async () => {
    await i18n.changeLanguage('zh-CN')
    const onRefresh = vi.fn()
    const view = render(<EditableField onRefresh={onRefresh} stale />)

    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('saved-custom')
    expect(screen.getByRole('status')).toHaveTextContent('运行时设置已更改。请刷新可用值。')
    expect(screen.getByRole('button', { name: '刷新可用值' })).toBeInTheDocument()

    view.rerender(<EditableField onRefresh={onRefresh} isLoading />)
    expect(screen.getByRole('status')).toHaveTextContent('正在加载可用值…')
  })
})
