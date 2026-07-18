import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { DetailShell } from '@/components/layout/DetailShell'

describe('DetailShell', () => {
  afterEach(cleanup)

  it('keeps its scroll container constrained by the parent layout', () => {
    const { container } = render(
      <DetailShell title="Manage group">
        <div>Settings content</div>
      </DetailShell>,
    )

    expect(container.firstElementChild).toHaveClass('h-full', 'min-h-0', 'overflow-y-auto')
  })
})
