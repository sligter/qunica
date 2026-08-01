import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
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

async function renderPage(actions: AppActionRead[]) {
  fetchJson.mockResolvedValue(actions)
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
})
