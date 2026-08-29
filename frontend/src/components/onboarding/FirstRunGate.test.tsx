import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { FirstRunGate } from '@/components/onboarding/FirstRunGate'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { useAuthStore } from '@/stores/authStore'
import type { AssistantRead, LLMProviderRead, SystemSettingsRead } from '@/types/api'

const fetchJson = vi.hoisted(() => vi.fn())
vi.mock('@/lib/api-v2/client', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-v2/client')>(
    '@/lib/api-v2/client',
  )
  return { ...actual, fetchJson }
})

const PROVIDER: LLMProviderRead = {
  id: 'provider-1',
  name: 'OpenAI',
  kind: 'openai-compatible',
  base_url: 'https://api.openai.com/v1',
  api_key_masked: 'sk-••••',
  headers_masked: {},
  user_agent: null,
  default_model: 'gpt-5-mini',
  context_window_tokens: null,
  context_output_reserve_ratio: null,
  description: null,
  reasoning_passback: false,
  models: [{
    id: 'gpt-5-mini',
    context_window_tokens: null,
    context_output_reserve_ratio: null,
  }],
  status: 'active',
  created_at: '2026-08-29T00:00:00Z',
}

const SETTINGS: SystemSettingsRead = {
  id: 'settings-1',
  owner_id: 'owner-1',
  appearance: 'system',
  language: 'en-US',
  reply_insert_mode: 'instant',
  assistant_enabled: true,
  assistant_auto_approve: false,
  onboarding_completed: false,
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
  created_at: '2026-08-29T00:00:00Z',
  updated_at: '2026-08-29T00:00:00Z',
}

async function renderGate(provider: LLMProviderRead | null = PROVIDER, root: string | null = null) {
  let settings = { ...SETTINGS, group_workspace_root: root }
  let assistant: AssistantRead = {
    agent_id: 'assistant-1',
    chat_id: 'chat-1',
    provider_id: null,
    model: null,
    provider_configured: false,
  }

  fetchJson.mockImplementation((path: string, options?: { method?: string; body?: unknown }) => {
    if (path === '/settings/system' && options?.method === 'PATCH') {
      const patch = options.body as Partial<SystemSettingsRead>
      settings = { ...settings, ...patch }
      if (patch.group_workspace_root === 'D:/Qunica') {
        settings.group_workspace_root = '\\\\?\\D:\\Qunica'
      }
      return Promise.resolve(settings)
    }
    if (path === '/settings/system') return Promise.resolve(settings)
    if (path === '/llm-providers') return Promise.resolve(provider ? [provider] : [])
    if (path === '/assistant' && options?.method === 'PATCH') {
      const body = options.body as { llm_provider_id: string; model: string }
      assistant = {
        ...assistant,
        provider_id: body.llm_provider_id,
        model: body.model,
        provider_configured: true,
      }
      return Promise.resolve(assistant)
    }
    if (path === '/assistant') return Promise.resolve(assistant)
    return Promise.reject(new Error(`Unexpected request: ${path}`))
  })

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
          <Routes>
            <Route element={<FirstRunGate />}>
              <Route path="/" element={<div>Qunica workspace</div>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('FirstRunGate', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'test-token' })
    fetchJson.mockReset()
  })

  afterEach(cleanup)

  it('saves the working root, provider, and explicit model before entering Qunica', async () => {
    const user = userEvent.setup()
    await renderGate()

    const root = await screen.findByLabelText('Working root')
    await user.type(root, 'D:/Qunica')
    await user.click(screen.getByRole('button', { name: 'Continue' }))

    await user.click(await screen.findByRole('button', { name: /OpenAI/ }))
    expect(await screen.findByRole('radio', { name: /gpt-5-mini/ })).toBeChecked()
    await user.click(screen.getByRole('button', { name: 'Continue' }))

    expect(await screen.findByRole('heading', { name: 'Your workbench is ready' })).toBeVisible()
    expect(screen.getByText('D:\\Qunica')).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'Enter Qunica' }))
    expect(await screen.findByText('Qunica workspace')).toBeVisible()

    expect(fetchJson).toHaveBeenCalledWith(
      '/assistant',
      expect.objectContaining({
        method: 'PATCH',
        body: { llm_provider_id: 'provider-1', model: 'gpt-5-mini' },
      }),
    )
    await waitFor(() => expect(fetchJson).toHaveBeenCalledWith(
      '/settings/system',
      expect.objectContaining({
        method: 'PATCH',
        body: { onboarding_completed: true, language: 'en-US' },
      }),
    ))
  })

  it('embeds provider creation instead of opening the resource library', async () => {
    await renderGate(null, 'D:/Qunica')

    expect(await screen.findByLabelText('Name')).toBeVisible()
    expect(screen.queryByRole('link')).toBeNull()
    expect(screen.getByText(/never detours through the resource library/i)).toBeVisible()
  })
})
