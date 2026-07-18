import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'

describe('DialogContent close label', () => {
  afterEach(cleanup)

  it('uses Close by default', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>Title</DialogTitle>
          <DialogDescription>Description</DialogDescription>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByRole('button', { name: 'Close' })).toBeVisible()
  })

  it('accepts a localized close label', () => {
    render(
      <Dialog open>
        <DialogContent closeLabel="关闭">
          <DialogTitle>Title</DialogTitle>
          <DialogDescription>Description</DialogDescription>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })
})
