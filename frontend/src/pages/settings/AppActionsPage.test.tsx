import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { AppActionsPage } from '@/pages/settings/AppActionsPage'
import { useAuthStore } from '@/stores/authStore'
import type { AppActionRead } from '@/types/api'

const fetchJson = vi.hoisted(() => vi.fn())
vi.mock('@/lib/api-v2/client', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-v2/client')>(
    '@/lib/api-v2/client',
  )
  return { ...actual, fetchJson }
})

function action(overrides: Partial<AppActionRead>): AppActionRead {
  return {
    id: 'action-1',
    conversation_id: 'chat-1',
    target_kind: 'agent',
    action: 'create',
    target_id: null,
    summary: 'Create agent "Researcher"',
    status: 'pending',
    result_json: null,
    created_at: '2026-08-01T10:00:00Z',
    resolved_at: null,
    ...overrides,
  }
}

async function renderPage(
  actions: AppActionRead[],
  hasMore = false,
  response?: (path: string, options?: { method?: string }) => unknown,
) {
  if (response) fetchJson.mockImplementation(response)
  else fetchJson.mockResolvedValue({ items: actions, has_more: hasMore })
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AppActionsPage />
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('AppActionsPage', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'test-token' })
    fetchJson.mockReset()
  })

  afterEach(cleanup)

  it('distinguishes every terminal state and keeps pending rows actionable', async () => {
    await renderPage([
      action({ id: 'a1', status: 'pending' }),
      action({ id: 'a2', status: 'applied', summary: 'Create skill "Review"' }),
      action({ id: 'a3', status: 'rejected', summary: 'Update group "Team"' }),
    ])

    expect(await screen.findByText('Create agent "Researcher"')).toBeVisible()
    expect(screen.getByText('Applied')).toBeVisible()
    expect(screen.getByText('Rejected')).toBeVisible()
    // Only the pending row offers a decision.
    expect(screen.getAllByRole('button', { name: 'Approve' })).toHaveLength(1)
  })

  it('shows why a failed apply failed', async () => {
    await renderPage([
      action({
        status: 'failed',
        result_json: JSON.stringify({
          error: 'workspace_id does not reference a workspace',
        }),
      }),
    ])

    // Without the reason, a failed row is indistinguishable from a bug.
    expect(
      await screen.findByText(/workspace_id does not reference a workspace/),
    ).toBeVisible()
  })

  it('says so when the assistant has proposed nothing', async () => {
    await renderPage([])
    expect(await screen.findByText(/has not proposed any changes/i)).toBeVisible()
  })

  it('moves between history pages', async () => {
    const user = userEvent.setup()
    const first = action({ id: 'a1', status: 'applied', summary: 'Create agent "First"' })
    const second = action({ id: 'a2', status: 'applied', summary: 'Create agent "Second"' })

    await renderPage([first], true, (path: string) =>
      path.includes('skip=50')
        ? { items: [second], has_more: false }
        : { items: [first], has_more: true },
    )

    expect(await screen.findByText('Create agent "First"')).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => expect(screen.getByText('Create agent "Second"')).toBeVisible())
    expect(fetchJson).toHaveBeenCalledWith('/app-actions?limit=50&skip=50', expect.anything())

    await user.click(screen.getByRole('button', { name: 'Previous' }))
    await waitFor(() => expect(screen.getByText('Create agent "First"')).toBeVisible())
  })

  it('deletes a resolved history entry after confirmation', async () => {
    const user = userEvent.setup()
    const item = action({ id: 'action-1', status: 'applied' })
    let deleted = false

    await renderPage([item], false, (_path, options) => {
      if (options?.method === 'DELETE') {
        deleted = true
        return undefined
      }
      return { items: deleted ? [] : [item], has_more: false }
    })

    expect(await screen.findByText(item.summary)).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'Delete history entry' }))
    await user.click(screen.getByRole('button', { name: 'Delete' }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/app-actions/action-1',
        expect.objectContaining({ method: 'DELETE' }),
      ),
    )
    expect(await screen.findByText(/has not proposed any changes/i)).toBeVisible()
  })

  it('clears resolved history in one confirmed action', async () => {
    const user = userEvent.setup()
    const pending = action({ id: 'pending', summary: 'Pending action' })
    const applied = action({ id: 'applied', status: 'applied', summary: 'Applied action' })
    let cleared = false

    await renderPage([applied, pending], false, (path, options) => {
      if (path === '/app-actions' && options?.method === 'DELETE') {
        cleared = true
        return undefined
      }
      return { items: cleared ? [pending] : [applied, pending], has_more: false }
    })

    expect(await screen.findByText('Applied action')).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'Clear history' }))
    const dialog = screen.getByRole('alertdialog')
    await user.click(within(dialog).getByRole('button', { name: 'Clear history' }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/app-actions',
        expect.objectContaining({ method: 'DELETE' }),
      ),
    )
    await waitFor(() => expect(screen.queryByText('Applied action')).not.toBeInTheDocument())
    expect(screen.getByText('Pending action')).toBeVisible()
  })
})
