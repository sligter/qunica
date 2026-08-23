import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage'
import { useAuthStore } from '@/stores/authStore'
import type { SystemSettingsRead } from '@/types/api'

const settings: SystemSettingsRead = {
  id: 'settings-1',
  owner_id: 'user-1',
  appearance: 'system',
  language: 'en-US',
  reply_insert_mode: 'instant',
  assistant_enabled: true,
  assistant_auto_approve: false,
  group_workspace_root: null,
  shell_preference: 'auto',
  web_search_provider: 'tavily',
  tavily_api_key_configured: false,
  tavily_search_url: 'https://api.tavily.com/search',
  tavily_max_results: 5,
  tavily_search_depth: 'basic',
  tavily_include_answer: true,
  tavily_include_raw_content: false,
  media_base_url: 'https://api.openai.com',
  media_api_key_configured: false,
  image_generation_model: null,
  image_generation_endpoint: '/v1/images/generations',
  video_generation_model: null,
  video_generation_endpoint: '/v1/videos',
  video_status_endpoint: '/v1/videos/{id}',
  video_content_endpoint: '/v1/videos/{id}/content',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
}

function jsonResponse(body: unknown, init?: ResponseInit) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
}

async function renderSettingsPage() {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })

  render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
          })
        }
      >
        <SystemSettingsPage />
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('SystemSettingsPage preferences', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    localStorage.clear()
  })

  it('changes language optimistically and rolls back after a failed save', async () => {
    useAuthStore.setState({ token: 'token' })
    let rejectPatch!: (reason?: unknown) => void
    const patchResponse = new Promise<Response>((_resolve, reject) => {
      rejectPatch = reject
    })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockReturnValueOnce(patchResponse)
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()

    expect(await screen.findByRole('heading', { name: 'System settings' })).toBeVisible()
    const chinese = screen.getByRole('radio', { name: '中文' })
    await waitFor(() => expect(chinese).toBeEnabled())
    await user.click(chinese)

    expect(screen.getByRole('heading', { name: '系统设置' })).toBeVisible()
    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ language: 'zh-CN' })
    rejectPatch(new Error('offline'))
    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'English' })).toHaveAttribute(
        'aria-checked',
        'true',
      )
    })
    expect(screen.getByRole('alert')).toHaveTextContent('Language update failed.')
  })

  it('saves whether the assistant launcher is enabled', async () => {
    useAuthStore.setState({ token: 'token' })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockResolvedValueOnce(jsonResponse({ ...settings, assistant_enabled: false }))
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()

    const toggle = await screen.findByRole('switch', { name: 'Enable assistant' })
    await waitFor(() => expect(toggle).toBeEnabled())
    expect(toggle).toBeChecked()
    await user.click(toggle)

    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ assistant_enabled: false })
    await waitFor(() => expect(toggle).not.toBeChecked())
  })

  it('saves the integrated terminal shell and rolls back after a failed save', async () => {
    useAuthStore.setState({ token: 'token' })
    let rejectPatch!: (reason?: unknown) => void
    const patchResponse = new Promise<Response>((_resolve, reject) => {
      rejectPatch = reject
    })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockReturnValueOnce(patchResponse)
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderSettingsPage()

    const select = await screen.findByLabelText('Integrated terminal shell')
    await waitFor(() => expect(select).toBeEnabled())
    expect(select).toHaveValue('auto')
    await user.selectOptions(select, 'bash')

    const [, patchInit] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(patchInit.method).toBe('PATCH')
    expect(JSON.parse(String(patchInit.body))).toEqual({ shell_preference: 'bash' })

    // A failed save must not leave the UI claiming a shell the backend never
    // stored: the next terminal would start under the old one.
    rejectPatch(new Error('offline'))
    await waitFor(() => expect(select).toHaveValue('auto'))
    expect(screen.getByRole('alert')).toBeVisible()
  })
})
