import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/lib/api-v2/client'
import { AssistantApprovalCard } from '@/components/assistant/AssistantApprovalCard'
import { ProviderCreatePage } from '@/pages/providers/ProviderCreatePage'
import { McpServerCreatePage } from '@/pages/mcp/McpServerCreatePage'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { useAuthStore } from '@/stores/authStore'
import type { PendingAppAction } from '@/lib/appActions'

const fetchJson = vi.hoisted(() => vi.fn())
const autoApprove = vi.hoisted(() => ({ enabled: false }))
const savedAction = vi.hoisted(() => ({ status: 'pending' }))
const desktop = vi.hoisted(() => ({ enabled: false, openLibraryWindow: vi.fn() }))
vi.mock('@/lib/desktop', async () => ({
  ...await vi.importActual<typeof import('@/lib/desktop')>('@/lib/desktop'),
  isDesktopRuntime: () => desktop.enabled,
  openLibraryWindow: desktop.openLibraryWindow,
}))
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

async function renderCard(action: PendingAppAction = PENDING, initialEntry = '/') {
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
        <MemoryRouter initialEntries={[initialEntry]}>
          <Routes>
            <Route path="/" element={<AssistantApprovalCard action={action} />} />
            <Route path="/providers/new" element={<ProviderCreatePage />} />
            <Route path="/mcp-servers/new" element={<McpServerCreatePage />} />
          </Routes>
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
    desktop.enabled = false
    desktop.openLibraryWindow.mockReset().mockResolvedValue(undefined)
    fetchJson.mockImplementation((path: string) => {
      if (path === '/settings/system') {
        return Promise.resolve({ assistant_auto_approve: autoApprove.enabled })
      }
      if (path === '/app-actions/action-1') {
        return Promise.resolve({ id: 'action-1', status: savedAction.status, result_json: savedAction.status === 'failed' ? JSON.stringify({ error: 'Execution failed' }) : null })
      }
      if (path === '/app-actions' || path.startsWith('/app-actions?')) {
        return Promise.resolve({
          items: Array.from({ length: 50 }, (_, index) => ({ id: `newer-${index}`, status: 'applied' })),
          has_more: true,
        })
      }
      return Promise.resolve({ id: 'action-1', status: 'applied' })
    })
  })

  afterEach(cleanup)

  it('shows the staged summary with both actions available', async () => {
    await renderCard()
    expect(screen.getByText('Create agent "Researcher"')).toBeVisible()
    expect(await screen.findByRole('button', { name: 'Confirm and run' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Do not run' })).toBeEnabled()
  })

  it('says plainly that nothing changes before confirmation', async () => {
    await renderCard()
    // Without this the user reads a summary in past tense and assumes it is done.
    expect(await screen.findByText(/nothing changes until you confirm/i)).toBeVisible()
  })

  it('approves once and refreshes the affected list', async () => {
    const user = userEvent.setup()
    const { invalidated } = await renderCard()

    await user.click(await screen.findByRole('button', { name: 'Confirm and run' }))

    expect(fetchJson).toHaveBeenCalledWith(
      '/app-actions/action-1/approve',
      expect.objectContaining({ method: 'POST' }),
    )
    await waitFor(() => expect(invalidated).toContainEqual(['agents']))
  })

  it('automatically approves pending beyond the first history page', async () => {
    autoApprove.enabled = true
    await renderCard()

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/app-actions/action-1/approve',
        expect.objectContaining({ method: 'POST' }),
      ),
    )
    expect(await screen.findByText('Applied')).toBeVisible()
    expect(fetchJson.mock.calls.some(([path]) => path === '/app-actions' || path.startsWith('/app-actions?'))).toBe(false)
  })

  it('cannot be resubmitted once resolved', async () => {
    const user = userEvent.setup()
    await renderCard()

    await user.click(await screen.findByRole('button', { name: 'Confirm and run' }))

    await waitFor(() => expect(screen.getByText('Applied')).toBeVisible())
    expect(screen.queryByRole('button', { name: 'Confirm and run' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Do not run' })).toBeNull()
  })

  it('restores a terminal card outside the recent 50 records after remount', async () => {
    savedAction.status = 'applied'
    await renderCard()

    expect(await screen.findByText('Applied')).toBeVisible()
    cleanup()
    await renderCard()
    expect(await screen.findByText('Applied')).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Confirm and run' })).toBeNull()
    expect(screen.queryByText(/nothing changes until you confirm/i)).toBeNull()
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

  it('polls approved to failed and displays the durable failure reason', async () => {
    savedAction.status = 'approved'
    await renderCard()
    expect(await screen.findByText('Running…')).toBeVisible()
    savedAction.status = 'failed'
    expect(await screen.findByText('Execution failed', {}, { timeout: 2_500 })).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Confirm and run' })).toBeNull()
  })

  it('shows deleted history as unavailable without automatically approving', async () => {
    autoApprove.enabled = true
    const fallback = fetchJson.getMockImplementation()!
    fetchJson.mockImplementation((path: string) => path === '/app-actions/action-1'
      ? Promise.reject(new ApiError(404, 'not_found', 'action not found')) : fallback(path))
    await renderCard()
    expect(await screen.findByText('This action record is no longer available')).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Confirm and run' })).toBeNull()
    expect(fetchJson.mock.calls.some(([, options]) => options?.method === 'POST')).toBe(false)
  })

  it('does not offer approval when status lookup fails', async () => {
    autoApprove.enabled = true
    const fallback = fetchJson.getMockImplementation()!
    fetchJson.mockImplementation((path: string) => path === '/app-actions/action-1'
      ? Promise.reject(new ApiError(500, 'internal', 'database error')) : fallback(path))
    await renderCard()
    expect(await screen.findByText('Could not load action status')).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Confirm and run' })).toBeNull()
    expect(fetchJson.mock.calls.some(([, options]) => options?.method === 'POST')).toBe(false)
  })

  it('surfaces the reason when the apply fails', async () => {
    const user = userEvent.setup()
    const { ApiError } = await import('@/lib/api-v2/client')
    const fallback = fetchJson.getMockImplementation()!
    fetchJson.mockImplementation((path: string) => path.endsWith('/approve')
      ? Promise.reject(new ApiError(422, 'app_action_failed', 'workspace_id does not reference a workspace'))
      : fallback(path))
    const { invalidated } = await renderCard()

    await user.click(await screen.findByRole('button', { name: 'Confirm and run' }))

    // A silent no-op would leave the user believing the change went through.
    expect(
      await screen.findByText(/workspace_id does not reference a workspace/),
    ).toBeVisible()
    expect(invalidated).toContainEqual(['app-actions'])
  })

  it('rejects without applying anything', async () => {
    const user = userEvent.setup()
    const fallback = fetchJson.getMockImplementation()!
    fetchJson.mockImplementation((path: string) => path.endsWith('/reject')
      ? Promise.resolve({ id: 'action-1', status: 'rejected' }) : fallback(path))
    await renderCard()

    await user.click(await screen.findByRole('button', { name: 'Do not run' }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/app-actions/action-1/reject',
        expect.objectContaining({ method: 'POST' }),
      ),
    )
    expect(await screen.findByText('Rejected')).toBeVisible()
  })

  it.each([false, true])('prefills a provider without credentials or saving (desktop=%s)', async (native) => {
    desktop.enabled = native
    const user = userEvent.setup()
    const action = {
      route: '/providers/new',
      fields: { name: 'Primary', kind: 'anthropic', base_url: 'https://models.example.com',
        default_model: 'claude-model', api_key: 'secret', headers: { Authorization: 'secret' } },
    }
    const view = await renderCard(action)
    const link = screen.getByRole('link', { name: /open the form/i })
    expect(link.getAttribute('href')).not.toContain('secret')
    expect(screen.queryByText('secret')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Confirm and run' })).toBeNull()
    await user.click(link)
    if (native) {
      expect(desktop.openLibraryWindow).toHaveBeenCalledOnce()
      const route = desktop.openLibraryWindow.mock.calls[0][0] as string
      view.unmount()
      // The existing native bridge opens this URL in the library's SPA.
      await renderCard(action, route)
    }
    expect(await screen.findByLabelText('Name')).toHaveValue('Primary')
    expect(screen.getByDisplayValue('https://models.example.com')).toBeVisible()
    expect(screen.getByLabelText('API key')).toHaveValue('')
    expect(screen.getAllByDisplayValue('claude-model').length).toBeGreaterThan(0)
    expect(fetchJson.mock.calls.filter(([, options]) => ['POST', 'PATCH'].includes(options?.method))).toEqual([])
  })

  it.each([
    [false, 'http'], [false, 'sse'], [true, 'http'], [true, 'sse'],
  ])('prefills an MCP form without headers or saving (desktop=%s, transport=%s)', async (native, transport) => {
    desktop.enabled = native
    const user = userEvent.setup()
    const action = {
      route: '/mcp-servers/new',
      fields: { name: 'Weather', transport, url: 'https://mcp.example.com/rpc', timeout_seconds: 30,
        headers: { Authorization: 'Bearer secret' }, env: { TOKEN: 'secret' }, command: 'unsafe' },
    }
    const view = await renderCard(action)
    await user.click(screen.getByRole('link', { name: /open the form/i }))
    if (native) {
      expect(desktop.openLibraryWindow).toHaveBeenCalledOnce()
      const route = desktop.openLibraryWindow.mock.calls[0][0] as string
      expect(route).not.toContain('secret')
      view.unmount()
      await renderCard(action, route)
    }
    expect(await screen.findByLabelText(/Name/)).toHaveValue('Weather')
    expect(screen.getByDisplayValue('https://mcp.example.com/rpc')).toBeVisible()
    expect(screen.getByRole('radio', { name: transport === 'sse' ? /SSE \(legacy\)/ : /Streamable HTTP/ })).toHaveAttribute('aria-checked', 'true')
    await user.click(screen.getByRole('button', { name: 'Add header' }))
    expect(screen.getByPlaceholderText('Bearer ...')).toHaveValue('')
    expect(screen.queryByDisplayValue('Bearer secret')).toBeNull()
    expect(fetchJson.mock.calls.filter(([, options]) => ['POST', 'PATCH'].includes(options?.method))).toEqual([])
  })
})
