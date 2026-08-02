import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { MediaSettingsPage } from '@/pages/settings/MediaSettingsPage'
import { useAuthStore } from '@/stores/authStore'
import type { SystemSettingsRead } from '@/types/api'

const settings: SystemSettingsRead = {
  id: 'settings-1',
  owner_id: 'user-1',
  appearance: 'system',
  language: 'en-US',
  assistant_auto_approve: false,
  group_workspace_root: null,
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
        <MediaSettingsPage />
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('MediaSettingsPage', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
  })

  it('saves shared credentials and independent image/video models', async () => {
    useAuthStore.setState({ token: 'token' })
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(settings))
      .mockResolvedValueOnce(
        jsonResponse({
          ...settings,
          media_api_key_configured: true,
          image_generation_model: 'image-model',
          video_generation_model: 'video-model',
        }),
      )
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()

    await renderPage()
    expect(await screen.findByRole('heading', { name: 'Media generation' })).toBeVisible()

    const apiKey = screen.getByLabelText('API key')
    await waitFor(() => expect(apiKey).toBeEnabled())
    await user.type(apiKey, 'secret-key')
    await user.type(screen.getByLabelText('Default model', { selector: '#media-image-model' }), 'image-model')
    await user.type(screen.getByLabelText('Default model', { selector: '#media-video-model' }), 'video-model')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    const [, init] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(init.method).toBe('PATCH')
    expect(JSON.parse(String(init.body))).toMatchObject({
      media_api_key: 'secret-key',
      image_generation_model: 'image-model',
      video_generation_model: 'video-model',
      image_generation_endpoint: '/v1/images/generations',
      video_generation_endpoint: '/v1/videos',
      video_status_endpoint: '/v1/videos/{id}',
      video_content_endpoint: '/v1/videos/{id}/content',
    })
  })
})
