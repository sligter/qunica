import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AssistantSettings } from '@/components/assistant/AssistantSettings'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { useAuthStore } from '@/stores/authStore'

const fetchJson = vi.hoisted(() => vi.fn())
vi.mock('@/lib/api-v2/client', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-v2/client')>(
    '@/lib/api-v2/client',
  )
  return { ...actual, fetchJson }
})

const PROVIDERS = [
  {
    id: 'provider-1',
    name: 'DeepSeek',
    kind: 'openai-compatible',
    default_model: 'deepseek-v4-flash',
    models: [{ id: 'deepseek-v4-flash' }, { id: 'deepseek-r1' }],
  },
  {
    id: 'provider-2',
    name: 'OpenAI',
    kind: 'openai-compatible',
    default_model: 'gpt-4o',
    models: [{ id: 'gpt-4o' }],
  },
]

async function renderSettings(
  assistant: Record<string, unknown> = {
    agent_id: 'agent-1',
    chat_id: 'chat-1',
    provider_id: 'provider-1',
    model: null,
    provider_configured: true,
  },
) {
  fetchJson.mockImplementation((path: string, options?: { method?: string; body?: unknown }) => {
    if (path === '/assistant' && options?.method === 'PATCH') {
      const body = options.body as { llm_provider_id: string | null; model: string | null }
      return Promise.resolve({
        ...assistant,
        provider_id: body.llm_provider_id,
        model: body.model,
        provider_configured: body.llm_provider_id !== null,
      })
    }
    if (path === '/assistant') return Promise.resolve(assistant)
    if (path === '/llm-providers') return Promise.resolve(PROVIDERS)
    return Promise.resolve([])
  })

  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const onClose = vi.fn()
  const view = render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AssistantSettings onClose={onClose} />
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
  return { ...view, onClose }
}

describe('AssistantSettings', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'test-token' })
    fetchJson.mockReset()
  })

  afterEach(cleanup)

  it('shows the currently bound provider as selected', async () => {
    await renderSettings()
    const select = (await screen.findByLabelText(/provider/i)) as HTMLSelectElement
    expect(select.value).toBe('provider-1')
  })

  it('changes the provider', async () => {
    const user = userEvent.setup()
    const { onClose } = await renderSettings()

    await user.selectOptions(await screen.findByLabelText(/provider/i), 'provider-2')
    await user.click(screen.getByRole('button', { name: /save/i }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/assistant',
        expect.objectContaining({
          method: 'PATCH',
          body: { llm_provider_id: 'provider-2', model: null },
        }),
      ),
    )
    expect(onClose).toHaveBeenCalled()
  })

  it('offers only the selected provider\'s models', async () => {
    const user = userEvent.setup()
    await renderSettings()

    // DeepSeek's two models are listed...
    expect(await screen.findByRole('option', { name: 'deepseek-r1' })).toBeInTheDocument()

    await user.selectOptions(screen.getByLabelText(/provider/i), 'provider-2')
    // ...and disappear once a provider that does not offer them is chosen.
    // Keeping them would let the user pin a model the provider will reject.
    expect(screen.queryByRole('option', { name: 'deepseek-r1' })).toBeNull()
    expect(screen.getByRole('option', { name: 'gpt-4o' })).toBeInTheDocument()
  })

  it('clears a model that the newly chosen provider does not offer', async () => {
    const user = userEvent.setup()
    await renderSettings({
      agent_id: 'agent-1',
      chat_id: 'chat-1',
      provider_id: 'provider-1',
      model: 'deepseek-r1',
      provider_configured: true,
    })

    await user.selectOptions(await screen.findByLabelText(/provider/i), 'provider-2')
    await user.click(screen.getByRole('button', { name: /save/i }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/assistant',
        expect.objectContaining({
          body: { llm_provider_id: 'provider-2', model: null },
        }),
      ),
    )
  })

  it('saves an explicitly chosen model', async () => {
    const user = userEvent.setup()
    await renderSettings()

    await user.selectOptions(await screen.findByLabelText(/model/i), 'deepseek-r1')
    await user.click(screen.getByRole('button', { name: /save/i }))

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/assistant',
        expect.objectContaining({
          body: { llm_provider_id: 'provider-1', model: 'deepseek-r1' },
        }),
      ),
    )
  })

  it('surfaces a rejected save instead of closing', async () => {
    const user = userEvent.setup()
    const { ApiError } = await import('@/lib/api-v2/client')
    const { onClose } = await renderSettings()
    fetchJson.mockRejectedValueOnce(
      new ApiError(400, 'invalid_input', 'model is not offered by that provider'),
    )

    await user.click(await screen.findByRole('button', { name: /save/i }))

    expect(await screen.findByText(/not offered by that provider/)).toBeVisible()
    expect(onClose).not.toHaveBeenCalled()
  })
})
