import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'

describe('SheetContent close label', () => {
  afterEach(cleanup)

  it('uses Close by default', () => {
    render(
      <Sheet open>
        <SheetContent>
          <SheetTitle>Title</SheetTitle>
          <SheetDescription>Description</SheetDescription>
        </SheetContent>
      </Sheet>,
    )

    expect(screen.getByRole('button', { name: 'Close' })).toBeVisible()
  })

  it('accepts a localized close label', () => {
    render(
      <Sheet open>
        <SheetContent closeLabel="关闭">
          <SheetTitle>Title</SheetTitle>
          <SheetDescription>Description</SheetDescription>
        </SheetContent>
      </Sheet>,
    )

    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })
})
