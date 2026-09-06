import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { CompactActions } from './CompactActions'
import { Dialog, DialogContent, DialogTitle, DialogTrigger } from '@/components/ui/dialog'

function viewport(compact: boolean) {
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: !compact, addEventListener: vi.fn(), removeEventListener: vi.fn() })))
}

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

describe('CompactActions', () => {
  it('keeps desktop actions directly available', () => {
    viewport(false)
    const { container } = render(<CompactActions label="More"><button>Open files</button></CompactActions>)
    expect(screen.getByRole('button', { name: 'Open files' })).toBeVisible()
    expect(container.querySelector('details')).toBeNull()
  })

  it('opens touch actions and dismisses on outside press or Escape', async () => {
    viewport(true)
    const user = userEvent.setup()
    const { container } = render(<CompactActions label="More"><button>Open files</button></CompactActions>)
    const summary = container.querySelector('summary')!
    const details = container.querySelector('details')!
    await user.click(summary)
    expect(details.open).toBe(true)
    fireEvent.pointerDown(document.body)
    expect(details.open).toBe(false)
    await user.click(summary)
    await user.keyboard('{Escape}')
    expect(details.open).toBe(false)
    expect(summary).toHaveFocus()
  })

  it('closes the touch menu without unmounting a dialog opened by an action', async () => {
    viewport(true)
    const user = userEvent.setup()
    const { container } = render(<CompactActions label="More">
      <Dialog>
        <DialogTrigger asChild><button>Create task</button></DialogTrigger>
        <DialogContent aria-describedby={undefined}><DialogTitle>New task</DialogTitle></DialogContent>
      </Dialog>
    </CompactActions>)
    await user.click(container.querySelector('summary')!)
    await user.click(screen.getByRole('button', { name: 'Create task' }))
    expect(container.querySelector('details')!.open).toBe(false)
    expect(screen.getByRole('dialog', { name: 'New task' })).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
