import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { SettingsOverlay } from '@/components/layout/SettingsOverlay'
import { UnsavedChangesProvider } from '@/components/layout/UnsavedChangesProvider'
import '@/i18n'
import { useUnsavedChangesGuard } from '@/hooks/useUnsavedChangesGuard'

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
})
