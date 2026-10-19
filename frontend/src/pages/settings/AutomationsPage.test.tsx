import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { AutomationsPage } from '@/pages/settings/AutomationsPage'
import { useAuthStore } from '@/stores/authStore'
import type { AutomationHistoryItem, AutomationRead } from '@/types/api'

const fetchJson = vi.hoisted(() => vi.fn())

vi.mock('@/lib/api-v2/client', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-v2/client')>(
    '@/lib/api-v2/client',
  )
  return { ...actual, fetchJson }
})

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({
    data: [
      { id: 'group-1', name: 'Studio' },
      { id: 'group-2', name: 'Second studio' },
    ],
    isLoading: false,
    error: null,
  }),
}))

function automation(overrides: Partial<AutomationRead> = {}): AutomationRead {
  return {
    id: 'automation-1',
    group_id: 'group-1',
    name: 'Morning check',
    prompt: 'Summarize the repo',
    trigger: { kind: 'cron', expression: '0 9 * * *' },
    timezone: 'UTC',
    missed_policy: 'coalesce_latest',
    budget: { max_agent_steps: 4, max_total_tokens: 32000, turn_timeout_seconds: 60 },
    enabled: true,
    version: 3,
    next_run_at: Math.floor(Date.now() / 1000) + 3600,
    preview: [],
    hook_id: 'hook-1',
    hook_configured: false,
    latest: {
      occurrence_id: 'occurrence-1',
      job_id: 'job-1',
      status: 'succeeded',
      error: null,
    },
    permissions: [],
    created_at: '2026-10-18T00:00:00Z',
    updated_at: '2026-10-18T00:00:00Z',
    ...overrides,
  }
}

function historyItem(overrides: Partial<AutomationHistoryItem> = {}): AutomationHistoryItem {
  return {
    id: 'occurrence-1',
    version: 3,
    source: 'timer',
    scheduled_at: 1_760_000_000,
    job_id: 'job-1',
    thread_id: 'thread-1',
    status: 'succeeded',
    error: null,
    created_at: '2026-10-18T00:00:00Z',
    ...overrides,
  }
}

async function renderPage(tasks: AutomationRead[] = [automation()]) {
  fetchJson.mockImplementation((path: string, options?: { method?: string }) => {
    if (path.startsWith('/automations/preview')) {
      return Promise.resolve({ preview: ['2026-10-19T09:00:00+00:00'] })
    }
    if (path.endsWith('/history')) {
      return Promise.resolve({ items: [historyItem()] })
    }
    if (path.endsWith('/secret')) {
      return Promise.resolve({ hook_id: 'hook-1', secret: 'one-time-secret-value', version: 4 })
    }
    if (path.endsWith('/toggle') || options?.method === 'PATCH') {
      return Promise.resolve({ ...tasks[0], enabled: false, version: 4 })
    }
    if (options?.method === 'POST' && path === '/automations') {
      return Promise.resolve({
        ...automation(),
        id: 'automation-2',
        name: 'New task',
        version: 1,
      })
    }
    if (path.startsWith('/automations?')) {
      const groupId = new URLSearchParams(path.split('?')[1]).get('group_id')
      return Promise.resolve({
        items: groupId ? tasks.filter((task) => task.group_id === groupId) : tasks,
      })
    }
    return Promise.resolve({ items: tasks })
  })

  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, refetchInterval: false } },
  })
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AutomationsPage />
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('AutomationsPage', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'test-token' })
    fetchJson.mockReset()
  })

  afterEach(cleanup)

  it('lists a scheduled task with its group, trigger and latest result', async () => {
    await renderPage()
    expect(await screen.findByText('Morning check')).toBeVisible()
    expect(screen.getByText(/Studio/)).toBeVisible()
    expect(screen.getByText(/Cron 0 9 \* \* \*/)).toBeVisible()
    expect(screen.getByText('Succeeded')).toBeVisible()
    expect(screen.getByRole('switch', { name: 'Pause' })).toBeChecked()
  })

  it('filters the list by group without dropping the other group', async () => {
    const user = userEvent.setup()
    await renderPage([
      automation(),
      automation({
        id: 'automation-2',
        group_id: 'group-2',
        name: 'Second task',
      }),
    ])
    expect(await screen.findByText('Morning check')).toBeVisible()
    await user.click(screen.getByRole('combobox', { name: 'Filter by group' }))
    await user.click(await screen.findByRole('option', { name: 'Second studio' }))
    expect(await screen.findByText('Second task')).toBeVisible()
    await waitFor(() => expect(screen.queryByText('Morning check')).not.toBeInTheDocument())
    expect(fetchJson).toHaveBeenCalledWith(
      '/automations?group_id=group-2',
      expect.anything(),
    )
  })

  it('sends the pause confirmation with the version the list rendered', async () => {
    const user = userEvent.setup()
    await renderPage()
    await user.click(await screen.findByRole('switch', { name: 'Pause' }))
    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/automations/automation-1/toggle',
        expect.objectContaining({
          method: 'POST',
          body: { enabled: false, expected_version: 3 },
        }),
      ),
    )
  })

  it('creates a cron task from the editor with the selected budget', async () => {
    const user = userEvent.setup()
    await renderPage()
    await user.click(await screen.findByRole('button', { name: 'New automation' }))
    await user.type(await screen.findByLabelText(/Name/), 'Nightly audit')
    await user.type(screen.getByLabelText(/Prompt/), 'Check the dependencies')
    await user.click(screen.getByRole('button', { name: 'Create automation' }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/automations',
        expect.objectContaining({
          method: 'POST',
          body: expect.objectContaining({
            group_id: 'group-1',
            name: 'Nightly audit',
            prompt: 'Check the dependencies',
            trigger: { kind: 'cron', expression: '0 9 * * *' },
            budget: {
              max_agent_steps: 8,
              max_total_tokens: 64000,
              turn_timeout_seconds: 300,
            },
            expected_version: null,
          }),
        }),
      ),
    )
  })

  it('previews the next runs before the schedule is saved', async () => {
    const user = userEvent.setup()
    await renderPage()
    await user.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(
      await screen.findByText(/Oct 19, 2026/, undefined, { timeout: 2_000 }),
    ).toBeVisible()
  })

  it('edits a GitHub event subscription and sends the normalized trigger', async () => {
    const user = userEvent.setup()
    await renderPage([
      automation({
        trigger: { kind: 'github', events: ['push'] },
        next_run_at: null,
      }),
    ])
    expect(await screen.findByText(/GitHub push/)).toBeVisible()
    await user.click(await screen.findByRole('button', { name: 'Edit' }))
    const events = await screen.findByLabelText('Event filter')
    expect(events).toHaveValue('push')
    expect(
      screen.getByText(/GitHub deliveries are event-driven/),
    ).toBeVisible()
    await user.clear(events)
    await user.type(events, 'push, pull_request')
    await user.click(screen.getByRole('button', { name: 'Save changes' }))
    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/automations/automation-1',
        expect.objectContaining({
          method: 'PATCH',
          body: expect.objectContaining({
            trigger: { kind: 'github', events: ['push', 'pull_request'] },
          }),
        }),
      ),
    )
  })

  it('shows a generated webhook secret once and labels it as such', async () => {
    const user = userEvent.setup()
    await renderPage([
      automation({
        trigger: { kind: 'webhook' },
        next_run_at: null,
        latest: null,
        hook_configured: false,
      }),
    ])
    await user.click(await screen.findByRole('button', { name: 'Webhook secret' }))
    await user.click(await screen.findByRole('button', { name: 'Generate secret' }))
    expect(await screen.findByText('one-time-secret-value')).toBeVisible()
    expect(
      screen.getByText(/Copy this now/),
    ).toBeVisible()
  })

  it('loads run history with source and status', async () => {
    const user = userEvent.setup()
    await renderPage()
    await user.click(await screen.findByRole('button', { name: 'History' }))
    const dialog = await screen.findByRole('dialog')
    expect(await within(dialog).findByText('Schedule')).toBeVisible()
    expect(within(dialog).getByText('Succeeded')).toBeVisible()
    expect(within(dialog).getByText(/Version 3/)).toBeVisible()
  })

  it('explains that a webhook has no calendar preview', async () => {
    const user = userEvent.setup()
    await renderPage([automation({ trigger: { kind: 'webhook' }, next_run_at: null })])
    await user.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(await screen.findByText(/Event-driven: there is no calendar preview/)).toBeVisible()
  })
})
