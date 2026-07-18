import { cleanup, render } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import { AppLayout } from '@/components/layout/AppLayout'

describe('AppLayout', () => {
  afterEach(cleanup)

  it('prevents the native context menu anywhere in the application surface', () => {
    const { container } = render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <Routes>
            <Route element={<AppLayout />}>
              <Route index element={<div>Application content</div>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    )

    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    expect(container.firstElementChild?.dispatchEvent(event)).toBe(false)
    expect(event.defaultPrevented).toBe(true)
  })
})
