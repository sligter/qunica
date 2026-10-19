import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { ConversationTriggersTab } from './ConversationTriggersTab'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import type { AutomationRead } from '@/types/api'

const mocks = vi.hoisted(() => ({ request: vi.fn(), items: [] as AutomationRead[] }))
vi.mock('@/lib/api-v2/client', async () => ({ ...await vi.importActual('@/lib/api-v2/client'), fetchJson: (...args: unknown[]) => mocks.request(...args) }))

function task(overrides: Partial<AutomationRead> = {}): AutomationRead {
  return { id: 'timer-1', group_id: 'group-1', name: 'Morning briefing', prompt: 'Summarize the repo', trigger: { kind: 'cron', expression: '0 9 * * *' }, timezone: 'Asia/Shanghai', missed_policy: 'coalesce_latest', budget: { max_agent_steps: 4, max_total_tokens: 32000, turn_timeout_seconds: 60 }, enabled: true, version: 3, next_run_at: 1_800_000_000, preview: [], hook_id: 'hook-1', hook_configured: false, latest: { occurrence_id: 'occ-1', job_id: 'job-1', status: 'succeeded', error: null }, permissions: [], created_at: '2026-10-19T00:00:00Z', updated_at: '2026-10-19T00:00:00Z', ...overrides }
}
beforeEach(async () => {
  await i18n.changeLanguage('en-US')
  useAuthStore.setState({ token: 'trigger-account' })
  mocks.items = [task(), task({ id: 'foreign', group_id: 'group-2', name: 'Another group schedule' })]
  mocks.request.mockReset().mockImplementation(async (path: string, options?: { body?: { enabled: boolean } }) => {
    if (path === '/groups/group-1') return { id: 'group-1', name: 'Studio' }
    if (path === '/automations?group_id=group-1') return { items: mocks.items }
    if (path.endsWith('/toggle')) {
      mocks.items[0] = { ...mocks.items[0], enabled: options!.body!.enabled, version: 4 }
      return mocks.items[0]
    }
    if (path.endsWith('/history')) return { items: [{ id: 'occ-1', version: 3, source: 'timer', scheduled_at: 1_800_000_000, job_id: 'job-1', thread_id: 'thread-1', status: 'succeeded', error: null, created_at: '2026-10-19T00:00:00Z' }] }
    if (path === '/automations/preview') return { preview: ['2026-10-20T09:00:00+08:00'] }
    return {}
  })
})
afterEach(() => { cleanup(); useAuthStore.setState({ token: null }) })
function page(scope: 'groups' | 'direct-chats' = 'groups') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ConversationTriggersTab scope={scope} conversationId="group-1" /></MemoryRouter></QueryClientProvider>)
  return userEvent.setup()
}

it('shows group schedules and pauses using the accepted version', async () => {
  const user = page()
  expect(await screen.findByRole('button', { name: 'Morning briefing' })).toBeVisible()
  expect(screen.queryByText('Another group schedule')).not.toBeInTheDocument()
  expect(screen.getByText('Asia/Shanghai', { exact: false })).toBeVisible()
  expect(mocks.request).toHaveBeenCalledWith('/automations?group_id=group-1', expect.anything())
  await user.click(screen.getByRole('switch', { name: 'Pause Morning briefing' }))
  await waitFor(() => expect(mocks.request).toHaveBeenCalledWith('/automations/timer-1/toggle', expect.objectContaining({ method: 'POST', body: { enabled: false, expected_version: 3 } })))
  expect(await screen.findByRole('switch', { name: 'Resume Morning briefing' })).not.toBeChecked()
})

it('creates in the current group and reuses the server schedule preview', async () => {
  const user = page()
  await screen.findByRole('button', { name: 'Morning briefing' })
  await user.click(screen.getByRole('button', { name: 'New automation' }))
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByRole('combobox', { name: 'Group' })).toHaveTextContent('Studio')
  await waitFor(() => expect(mocks.request).toHaveBeenCalledWith('/automations/preview', expect.objectContaining({ method: 'POST', body: expect.objectContaining({ trigger: { kind: 'cron', expression: '0 9 * * *' } }) })))
})

it('opens the existing execution history for the selected schedule', async () => {
  const user = page()
  await screen.findByRole('button', { name: 'Morning briefing' })
  await user.click(screen.getByRole('button', { name: 'History' }))
  const dialog = await screen.findByRole('dialog')
  expect(await within(dialog).findByText('Succeeded')).toBeVisible()
  expect(mocks.request).toHaveBeenCalledWith('/automations/timer-1/history', expect.anything())
})

it('does not load account-wide schedules from a direct chat', () => {
  page('direct-chats')
  expect(screen.getByText('Schedules belong to groups')).toBeVisible()
  expect(mocks.request).not.toHaveBeenCalled()
})

it('keeps the failed toggle visible and refreshes before another versioned action', async () => {
  const original = mocks.request.getMockImplementation()!
  mocks.request.mockImplementation((path: string, options?: unknown) => path.endsWith('/toggle') ? Promise.reject(new Error('configuration version changed')) : original(path, options))
  const user = page()
  await screen.findByRole('button', { name: 'Morning briefing' })
  await user.click(screen.getByRole('switch', { name: 'Pause Morning briefing' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('configuration version changed')
  await waitFor(() => expect(mocks.request.mock.calls.filter(([path]) => path === '/automations?group_id=group-1').length).toBeGreaterThan(1))
})
