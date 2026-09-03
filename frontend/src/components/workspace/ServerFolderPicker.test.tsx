import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ServerFolderPicker } from '@/components/workspace/ServerFolderPicker'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'

function listing(path: '' | 'demo') {
  return path === ''
    ? {
        root: '/workspaces',
        absolute_path: '/workspaces',
        relative_path: '',
        parent_relative_path: null,
        entries: [{
          name: 'demo',
          relative_path: 'demo',
          absolute_path: '/workspaces/demo',
        }],
        truncated: false,
      }
    : {
        root: '/workspaces',
        absolute_path: '/workspaces/demo',
        relative_path: 'demo',
        parent_relative_path: '',
        entries: [],
        truncated: false,
      }
}

describe('ServerFolderPicker', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    useAuthStore.setState({ token: 'owner-token' })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  it('browses and selects directories on the backend machine', async () => {
    const fetchMock = vi.fn<typeof fetch>().mockImplementation(async (input) => {
      const url = String(input)
      const path = new URL(url, 'http://localhost').searchParams.get('path')
      return new Response(JSON.stringify(listing(path === 'demo' ? 'demo' : '')), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onSelect = vi.fn()
    const onOpenChange = vi.fn()
    const user = userEvent.setup()

    render(
      <QueryClientProvider
        client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
      >
        <ServerFolderPicker open onOpenChange={onOpenChange} onSelect={onSelect} />
      </QueryClientProvider>,
    )

    await user.click(await screen.findByRole('button', { name: 'demo' }))
    expect(await screen.findByTitle('/workspaces/demo')).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'Use this folder' }))

    expect(onSelect).toHaveBeenCalledWith('/workspaces/demo', 'demo')
    expect(onOpenChange).toHaveBeenCalledWith(false)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    for (const [url, init] of fetchMock.mock.calls) {
      expect(String(url)).toContain('/api/v2/workspaces/directories?path=')
      expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer owner-token')
    }
  })
})
