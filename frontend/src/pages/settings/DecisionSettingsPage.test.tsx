import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { DecisionSettingsPage } from '@/pages/settings/DecisionSettingsPage'
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
  onboarding_completed: true,
  group_workspace_root: null,
  shell_preference: 'auto',
  web_search_provider: 'tavily',
  tavily_api_key_configured: false,
  tavily_search_url: 'https://api.tavily.com/search',
  tavily_max_results: 5,
  tavily_search_depth: 'basic',
  tavily_include_answer: true,
  tavily_include_raw_content: false,
  deepseek_api_key_configured: false,
  deepseek_search_url: 'https://api.deepseek.com/v1/tools/search',
  deepseek_fetch_url: 'https://api.deepseek.com/v1/tools/fetch',
  deepseek_max_results: 5,
  deepseek_need_content: false,
  media_base_url: 'https://api.openai.com',
  media_api_key_configured: false,
  image_generation_model: null,
  image_generation_endpoint: '/v1/images/generations',
  video_generation_model: null,
  video_generation_endpoint: '/v1/videos',
  video_status_endpoint: '/v1/videos/{id}',
  video_content_endpoint: '/v1/videos/{id}/content',
  decision_endpoint: 'https://openrouter.ai/api/alpha/decisions',
  decision_api_key_configured: false,
  decision_model: '~typesafe/jev-latest',
  decision_min_confidence: 0.7,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
}

function jsonResponse(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

async function renderPage() {
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
        client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
      >
        <DecisionSettingsPage />
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('DecisionSettingsPage', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
  })

  it('saves the endpoint, key, model and confidence floor', async () => {
    useAuthStore.setState({ token: 'token' })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockResolvedValueOnce(
        jsonResponse({
          ...settings,
          decision_api_key_configured: true,
          decision_min_confidence: 0.6,
        }),
      )
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderPage()
    expect(await screen.findByRole('heading', { name: 'Decision model' })).toBeVisible()
    // The switches live on each group, not here.
    expect(screen.queryByRole('switch')).toBeNull()
    expect(screen.getByText(/chosen per group/)).toBeVisible()

    const apiKey = screen.getByLabelText('API key')
    await waitFor(() => expect(apiKey).toBeEnabled())
    await user.type(apiKey, 'sk-or-secret')
    const confidence = screen.getByLabelText('Minimum confidence')
    await user.clear(confidence)
    await user.type(confidence, '0.6')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    const [, init] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(init.method).toBe('PATCH')
    expect(JSON.parse(String(init.body))).toEqual({
      decision_api_key: 'sk-or-secret',
      decision_endpoint: 'https://openrouter.ai/api/alpha/decisions',
      decision_model: '~typesafe/jev-latest',
      decision_min_confidence: 0.6,
    })
    expect(await screen.findByText('Ready')).toBeVisible()
  })

  it('tests the endpoint with the key typed but not yet saved', async () => {
    useAuthStore.setState({ token: 'token' })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockResolvedValueOnce(
        jsonResponse({
          ok: true,
          model: 'typesafe/jev-1.13',
          sample_probability: 0.91,
          input_tokens: 40,
          message: 'The decision endpoint answered.',
          dialect: 'ai_sdk_gateway',
        }),
      )
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderPage()
    const apiKey = await screen.findByLabelText('API key')
    await waitFor(() => expect(apiKey).toBeEnabled())
    await user.type(apiKey, 'sk-or-secret')
    await user.click(screen.getByRole('button', { name: 'Test connection' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    const [url, init] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(String(url)).toContain('/settings/decision/test')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toMatchObject({ api_key: 'sk-or-secret' })
    expect(await screen.findByRole('status')).toHaveTextContent('typesafe/jev-1.13')
    expect(screen.getByRole('status')).toHaveTextContent('via Vercel AI Gateway')
  })
})
