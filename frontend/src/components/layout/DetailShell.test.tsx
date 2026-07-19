import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { DetailShell } from '@/components/layout/DetailShell'

describe('DetailShell', () => {
  afterEach(cleanup)

  it('keeps long detail content in a constrained inner scroll container', () => {
    const { container } = render(
      <DetailShell title="Manage group">
        <div>Settings content</div>
      </DetailShell>,
    )

    expect(container.firstElementChild).toHaveClass('h-full', 'min-h-0', 'overflow-hidden')
    expect(container.firstElementChild?.lastElementChild).toHaveClass(
      'min-h-0',
      'flex-1',
      'overflow-y-auto',
    )
  })
})
