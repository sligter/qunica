import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AssistantApprovalCard } from '@/components/assistant/AssistantApprovalCard'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { useAuthStore } from '@/stores/authStore'
import type { PendingAppAction } from '@/lib/appActions'

const fetchJson = vi.hoisted(() => vi.fn())
const autoApprove = vi.hoisted(() => ({ enabled: false }))
const savedAction = vi.hoisted(() => ({ status: 'pending' }))
vi.mock('@/lib/api-v2/client', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-v2/client')>(
    '@/lib/api-v2/client',
  )
  return { ...actual, fetchJson }
})

const PENDING: PendingAppAction = {
  action_id: 'action-1',
  target_kind: 'agent',
  action: 'create',
  summary: 'Create agent "Researcher"',
}

async function renderCard(action: PendingAppAction = PENDING) {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const invalidated: unknown[] = []
  client.invalidateQueries = vi.fn(async (filters?: { queryKey?: unknown }) => {
    invalidated.push(filters?.queryKey)
  }) as unknown as typeof client.invalidateQueries

  const view = render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AssistantApprovalCard action={action} />
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
  return { ...view, invalidated }
}

describe('AssistantApprovalCard', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'test-token' })
    fetchJson.mockReset()
    autoApprove.enabled = false
    savedAction.status = 'pending'
    fetchJson.mockImplementation((path: string) => {
      if (path === '/settings/system') {
        return Promise.resolve({ assistant_auto_approve: autoApprove.enabled })
      }
      if (path === '/app-actions' || path.startsWith('/app-actions?')) {
        return Promise.resolve({
          items: [
            {
              id: 'action-1',
              target_kind: 'agent',
              action: 'create',
              summary: PENDING.summary,
              status: savedAction.status,
            },
          ],
          has_more: false,
        })
      }
      return Promise.resolve({ id: 'action-1', status: 'applied' })
    })
  })

  afterEach(cleanup)

  it('shows the staged summary with both actions available', async () => {
    await renderCard()
    expect(screen.getByText('Create agent "Researcher"')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Reject' })).toBeEnabled()
  })

  it('says plainly that nothing has changed yet', async () => {
    await renderCard()
    // Without this the user reads a summary in past tense and assumes it is done.
    expect(screen.getByText(/nothing has changed yet/i)).toBeVisible()
  })

  it('approves once and refreshes the affected list', async () => {
    const user = userEvent.setup()
    const { invalidated } = await renderCard()

    await user.click(screen.getByRole('button', { name: 'Approve' }))

    expect(fetchJson).toHaveBeenCalledWith(
      '/app-actions/action-1/approve',
      expect.objectContaining({ method: 'POST' }),
    )
    await waitFor(() => expect(invalidated).toContainEqual(['agents']))
  })

  it('automatically approves a pending action when the mode is enabled', async () => {
    autoApprove.enabled = true
    await renderCard()

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/app-actions/action-1/approve',
        expect.objectContaining({ method: 'POST' }),
      ),
    )
    expect(await screen.findByText('Applied')).toBeVisible()
  })

  it('cannot be resubmitted once resolved', async () => {
    const user = userEvent.setup()
    await renderCard()

    await user.click(screen.getByRole('button', { name: 'Approve' }))

    await waitFor(() => expect(screen.getByText('Applied')).toBeVisible())
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Reject' })).toBeNull()
  })

  it('restores an already applied card from its durable status', async () => {
    savedAction.status = 'applied'
    await renderCard()

    expect(await screen.findByText('Applied')).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
    expect(screen.queryByText(/nothing has changed yet/i)).toBeNull()
  })

  it('polls a running message action until it is applied', async () => {
    savedAction.status = 'approved'
    const { invalidated } = await renderCard({
      action_id: 'action-1',
      target_kind: 'chat',
      action: 'update',
      summary: 'Send a message',
    })

    expect(await screen.findByText('Running…')).toBeVisible()
    savedAction.status = 'applied'

    await waitFor(() => expect(screen.getByText('Applied')).toBeVisible(), {
      timeout: 2_500,
    })
    expect(invalidated).toContainEqual(['direct-chats'])
  })

  it('surfaces the reason when the apply fails', async () => {
    const user = userEvent.setup()
    const { ApiError } = await import('@/lib/api-v2/client')
    fetchJson.mockRejectedValue(
      new ApiError(422, 'app_action_failed', 'workspace_id does not reference a workspace'),
    )
    const { invalidated } = await renderCard()

    await user.click(screen.getByRole('button', { name: 'Approve' }))

    // A silent no-op would leave the user believing the change went through.
    expect(
      await screen.findByText(/workspace_id does not reference a workspace/),
    ).toBeVisible()
    expect(invalidated).toContainEqual(['app-actions'])
  })

  it('rejects without applying anything', async () => {
    const user = userEvent.setup()
    fetchJson.mockResolvedValue({ id: 'action-1', status: 'rejected' })
    await renderCard()

    await user.click(screen.getByRole('button', { name: 'Reject' }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/app-actions/action-1/reject',
        expect.objectContaining({ method: 'POST' }),
      ),
    )
    expect(await screen.findByText('Rejected')).toBeVisible()
  })

  it('renders a prefill hand-off as a link instead of an approval', async () => {
    await renderCard({
      route: '/providers/new',
      fields: { name: 'OpenAI' },
    })

    const link = screen.getByRole('link', { name: /open the form/i })
    expect(link).toHaveAttribute('href', '/providers/new')
    // A prefill is not a staged change, so approving it would be meaningless.
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
  })
})
