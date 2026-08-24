import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AssistantDockWindow } from '@/pages/assistant/AssistantDockWindow'
import { enUS } from '@/i18n/resources/en-US'
import { useAuthStore } from '@/stores/authStore'

const fetchJson = vi.hoisted(() => vi.fn())
const windowMocks = vi.hoisted(() => ({
  hide: vi.fn(),
  startDragging: vi.fn(),
}))
vi.mock('@/lib/api-v2/client', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-v2/client')>(
    '@/lib/api-v2/client',
  )
  return { ...actual, fetchJson }
})

vi.mock('@/components/chat/ConversationChatView', () => ({
  ConversationChatView: () => <div data-testid="assistant-chat">chat</div>,
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: windowMocks.hide,
    startDragging: windowMocks.startDragging,
  }),
}))

async function renderWindow(
  assistant: Record<string, unknown> | null = {
    agent_id: 'agent-1',
    chat_id: 'chat-1',
    provider_id: 'provider-1',
    provider_configured: true,
  },
  pending = false,
) {
  fetchJson.mockImplementation((path: string) => {
    if (path === '/assistant') {
      if (pending) return new Promise(() => undefined)
      return assistant ? Promise.resolve(assistant) : Promise.reject(new Error('nope'))
    }
    if (path === '/llm-providers') return Promise.resolve([])
    return Promise.resolve([])
  })

  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: 'en-US',
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS },
    interpolation: { escapeValue: false },
  })

  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <MemoryRouter>
          <AssistantDockWindow />
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('AssistantDockWindow', () => {
  beforeEach(() => {
    fetchJson.mockReset()
    windowMocks.hide.mockReset().mockResolvedValue(undefined)
    windowMocks.startDragging.mockReset().mockResolvedValue(undefined)
    useAuthStore.setState({ token: 'test-token' })
  })

  afterEach(cleanup)

  it('renders the compact chat without a terminal runtime', async () => {
    const { container } = await renderWindow()
    expect(await screen.findByTestId('assistant-chat')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Assistant settings' })).toBeVisible()
    expect(screen.getByRole('button', { name: 'Collapse assistant' })).toBeVisible()
    expect(container.querySelector('[data-testid="assistant-window-drag-handle"]')).toBeInTheDocument()

    fireEvent.pointerDown(screen.getByTestId('assistant-window-drag-handle'), {
      button: 0,
    })
    await waitFor(() => expect(windowMocks.startDragging).toHaveBeenCalledTimes(1))

    // Window controls stay clickable instead of starting another move gesture.
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Assistant settings' }), {
      button: 0,
    })
    expect(windowMocks.startDragging).toHaveBeenCalledTimes(1)

    fireEvent.click(screen.getByRole('button', { name: 'Collapse assistant' }))
    await waitFor(() => expect(windowMocks.hide).toHaveBeenCalledTimes(1))
  })

  it('falls back to setup when no provider is bound', async () => {
    await renderWindow({
      agent_id: 'agent-1',
      chat_id: 'chat-1',
      provider_id: null,
      provider_configured: false,
    })
    expect(await screen.findByText('Connect a model first')).toBeVisible()
  })

  it('keeps the native drag handle available while assistant data loads', async () => {
    await renderWindow(undefined, true)

    const handle = screen.getByTestId('assistant-window-drag-handle')
    expect(handle).toBeVisible()
    fireEvent.pointerDown(handle, { button: 0 })
    await waitFor(() => expect(windowMocks.startDragging).toHaveBeenCalledTimes(1))
  })
})
