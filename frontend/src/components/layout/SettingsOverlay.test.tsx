import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SettingsOverlay } from '@/components/layout/SettingsOverlay'
import { UnsavedChangesProvider } from '@/components/layout/UnsavedChangesProvider'
import '@/i18n'
import { useUnsavedChangesGuard } from '@/hooks/useUnsavedChangesGuard'

const DRAWER_WIDTH_KEY = 'qunica:layout:group-settings-drawer-width'

function DirtySource({ dirty }: { dirty: boolean }) {
  useUnsavedChangesGuard(dirty)
  return <input aria-label="Draft" />
}

function renderOverlay(dirty: boolean, onClose = vi.fn()) {
  render(
    <UnsavedChangesProvider>
      <SettingsOverlay label="Settings" onClose={onClose}>
        <DirtySource dirty={dirty} />
      </SettingsOverlay>
    </UnsavedChangesProvider>,
  )
  return onClose
}

describe('SettingsOverlay unsaved changes guard', () => {
  beforeEach(() => localStorage.removeItem(DRAWER_WIDTH_KEY))
  afterEach(cleanup)

  it('closes clean forms immediately and confirms before discarding dirty edits', () => {
    const cleanClose = renderOverlay(false)
    fireEvent.keyDown(screen.getByRole('dialog', { name: 'Settings' }), { key: 'Escape' })
    expect(cleanClose).toHaveBeenCalledOnce()

    cleanup()
    const dirtyClose = renderOverlay(true)
    fireEvent.keyDown(screen.getByRole('dialog', { name: 'Settings' }), { key: 'Escape' })
    expect(dirtyClose).not.toHaveBeenCalled()
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Discard unsaved changes?')

    fireEvent.click(screen.getByRole('button', { name: 'Discard and leave' }))
    expect(dirtyClose).toHaveBeenCalledOnce()
  })

  it('renders the group-management variant as a docked drawer', () => {
    const onClose = vi.fn()
    const { container } = render(
      <UnsavedChangesProvider>
        <SettingsOverlay
          label="Group settings"
          resizeLabel="Resize group settings"
          onClose={onClose}
          variant="drawer"
        >
          <div>Drawer content</div>
        </SettingsOverlay>
      </UnsavedChangesProvider>,
    )

    const dialog = screen.getByRole('dialog', { name: 'Group settings' })
    expect(dialog).toHaveAttribute('data-variant', 'drawer')
    expect(dialog).toHaveStyle({ width: '512px' })
    expect(screen.getByRole('separator', { name: 'Resize group settings' })).toHaveAttribute(
      'aria-valuenow',
      '512',
    )

    fireEvent.click(container.querySelector('.group-drawer-scrim')!)
    expect(onClose).toHaveBeenCalledOnce()
  })

  it('resizes the group drawer from its left edge and restores the saved width', () => {
    const view = render(
      <UnsavedChangesProvider>
        <SettingsOverlay
          label="Group settings"
          resizeLabel="Resize group settings"
          onClose={vi.fn()}
          variant="drawer"
        >
          <div>Drawer content</div>
        </SettingsOverlay>
      </UnsavedChangesProvider>,
    )

    fireEvent.pointerDown(screen.getByRole('separator'), { clientX: 520 })
    fireEvent.pointerMove(window, { clientX: 420 })
    fireEvent.pointerUp(window)

    expect(screen.getByRole('dialog')).toHaveStyle({ width: '612px' })
    expect(localStorage.getItem(DRAWER_WIDTH_KEY)).toBe('612')

    view.unmount()
    render(
      <UnsavedChangesProvider>
        <SettingsOverlay label="Group settings" onClose={vi.fn()} variant="drawer">
          <div>Drawer content</div>
        </SettingsOverlay>
      </UnsavedChangesProvider>,
    )
    expect(screen.getByRole('dialog')).toHaveStyle({ width: '612px' })
  })
})
