import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AssistantDock } from '@/components/assistant/AssistantDock'
import {
  ASSISTANT_PLACEMENT_KEY,
  MIN_DOCK_WIDTH,
} from '@/components/assistant/useAssistantDockPlacement'
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

// The chat surface pulls in the whole streaming stack; the dock's own
// behaviour is what these tests cover.
vi.mock('@/components/chat/ConversationChatView', () => ({
  ConversationChatView: ({ agentIsSystem }: { agentIsSystem?: boolean }) => (
    <div data-testid="assistant-chat" data-agent-is-system={agentIsSystem}>chat</div>
  ),
}))

async function renderDock(
  assistant: Record<string, unknown> | null = {
    agent_id: 'agent-1',
    chat_id: 'chat-1',
    provider_id: 'provider-1',
    provider_configured: true,
  },
  providers: Array<Record<string, unknown>> = [],
) {
  fetchJson.mockImplementation((path: string, options?: { method?: string }) => {
    if (path === '/assistant') {
      if (options?.method === 'PATCH') {
        return Promise.resolve({ ...assistant, provider_configured: true })
      }
      return assistant ? Promise.resolve(assistant) : Promise.reject(new Error('nope'))
    }
    if (path === '/llm-providers') return Promise.resolve(providers)
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
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AssistantDock />
        </MemoryRouter>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('AssistantDock', () => {
  beforeEach(() => {
    localStorage.clear()
    useAuthStore.setState({ token: 'test-token' })
    fetchJson.mockReset()
  })

  afterEach(() => {
    cleanup()
    localStorage.clear()
  })

  it('renders collapsed, with the expanded panel absent from the tree', async () => {
    await renderDock()
    expect(await screen.findByRole('button', { name: /assistant/i })).toBeVisible()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('expands on click and collapses on Escape, returning focus to the launcher', async () => {
    const user = userEvent.setup()
    await renderDock()

    await user.click(await screen.findByRole('button', { name: /assistant/i }))
    expect(await screen.findByRole('dialog')).toBeVisible()

    await user.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    // Focus must come back, or a keyboard user is stranded after collapsing.
    expect(screen.getByRole('button', { name: /assistant/i })).toHaveFocus()
  })

  it('does not render at all without a token', async () => {
    useAuthStore.setState({ token: null })
    await renderDock()
    expect(screen.queryByRole('button', { name: /assistant/i })).toBeNull()
  })

  it('lets HTML5 file drags pass through instead of swallowing them', async () => {
    const user = userEvent.setup()
    await renderDock()
    await user.click(await screen.findByRole('button', { name: /assistant/i }))

    const dialog = await screen.findByRole('dialog')
    // Workspace file drag-and-drop is a native HTML5 drag. If the dock marked
    // itself draggable or preventDefault'ed these, dropping a file anywhere
    // near it would silently stop working.
    expect(dialog.getAttribute('draggable')).not.toBe('true')
    const dragOver = new Event('dragover', { bubbles: true, cancelable: true })
    fireEvent(dialog, dragOver)
    expect(dragOver.defaultPrevented).toBe(false)
  })

  it('offers a provider to bind when one exists but none is bound', async () => {
    const user = userEvent.setup()
    // The exact dead end a user hits after configuring providers and agents:
    // everything looks set up, but the assistant has no provider of its own.
    await renderDock(
      { agent_id: 'agent-1', chat_id: 'chat-1', provider_id: null, provider_configured: false },
      [{ id: 'provider-1', name: 'DeepSeek', kind: 'openai-compatible', default_model: 'deepseek-v4-flash' }],
    )

    await user.click(await screen.findByRole('button', { name: /assistant/i }))

    const choice = await screen.findByRole('button', { name: /DeepSeek/ })
    await user.click(choice)

    await waitFor(() =>
      expect(fetchJson).toHaveBeenCalledWith(
        '/assistant',
        expect.objectContaining({
          method: 'PATCH',
          // Both fields always travel: the backend treats an omitted one as
          // "clear", so a partial send would silently drop the other.
          body: { llm_provider_id: 'provider-1', model: 'deepseek-v4-flash' },
        }),
      ),
    )
    // Binding must unblock the dock, not leave the user where they started.
    expect(await screen.findByTestId('assistant-chat')).toBeVisible()
  })

  it('embeds provider creation when there are none at all', async () => {
    const user = userEvent.setup()
    await renderDock(
      { agent_id: 'agent-1', chat_id: 'chat-1', provider_id: null, provider_configured: false },
      [],
    )
    await user.click(await screen.findByRole('button', { name: /assistant/i }))
    expect(await screen.findByLabelText('Name')).toBeVisible()
    expect(screen.queryByRole('link', { name: /provider/i })).toBeNull()
  })

  it('shows the chat once a provider is configured', async () => {
    const user = userEvent.setup()
    await renderDock()
    await user.click(await screen.findByRole('button', { name: /assistant/i }))
    expect(await screen.findByTestId('assistant-chat')).toHaveAttribute(
      'data-agent-is-system',
      'true',
    )
  })

  it('opens settings from the header and returns to the chat on cancel', async () => {
    const user = userEvent.setup()
    await renderDock()
    await user.click(await screen.findByRole('button', { name: /assistant/i }))
    expect(await screen.findByTestId('assistant-chat')).toBeVisible()

    // Reachable once a provider is bound — otherwise there is no way to change
    // the assistant's provider or model after the initial setup.
    await user.click(screen.getByRole('button', { name: /assistant settings/i }))
    expect(await screen.findByLabelText(/provider/i)).toBeVisible()
    expect(screen.queryByTestId('assistant-chat')).toBeNull()

    await user.click(screen.getByRole('button', { name: /cancel/i }))
    expect(await screen.findByTestId('assistant-chat')).toBeVisible()
  })

  it('sits below the dialog layer so dialogs opened from it are usable', async () => {
    const user = userEvent.setup()
    await renderDock()
    await user.click(await screen.findByRole('button', { name: /assistant/i }))
    const dialog = await screen.findByRole('dialog')

    // Radix dialogs portal to document.body at z-50, as siblings of the dock
    // rather than descendants. A dock above that renders over its own confirm
    // dialogs, leaving them visible but unclickable.
    const layer = (element: Element) => {
      const match = /(?:^|\s)z-\[(\d+)\]/.exec(element.className)
      return match ? Number(match[1]) : 0
    }
    expect(layer(dialog)).toBeLessThan(50)

    // The collapsed launcher is the other portalled surface.
    await user.click(screen.getByRole('button', { name: 'Collapse assistant' }))
    const launcher = await screen.findByRole('button', { name: 'Assistant' })
    expect(layer(launcher)).toBeLessThan(50)
  })

  it('remembers that it was left expanded', async () => {
    localStorage.setItem(
      ASSISTANT_PLACEMENT_KEY,
      JSON.stringify({ x: 100, y: 100, width: 380, height: 560, collapsed: false }),
    )
    await renderDock()
    expect(await screen.findByRole('dialog')).toBeVisible()
  })

  it('resizes from every edge and corner', async () => {
    localStorage.setItem(
      ASSISTANT_PLACEMENT_KEY,
      JSON.stringify({ x: 400, y: 200, width: 380, height: 560, collapsed: false }),
    )
    await renderDock()
    const dialog = await screen.findByRole('dialog')

    for (const direction of ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw']) {
      const handle = screen.getByTestId(`assistant-dock-resize-${direction}`)
      const before = { w: dialog.style.width, h: dialog.style.height }

      fireEvent.pointerDown(handle, { clientX: 600, clientY: 500, pointerId: 1, button: 0 })
      fireEvent.pointerMove(window, { clientX: 660, clientY: 560, pointerId: 1 })
      fireEvent.pointerUp(window, { clientX: 660, clientY: 560, pointerId: 1 })

      const changed =
        dialog.style.width !== before.w || dialog.style.height !== before.h
      expect(changed, `dragging ${direction} changed nothing`).toBe(true)
    }
  })

  it('stops at the minimum size instead of sliding the panel sideways', async () => {
    localStorage.setItem(
      ASSISTANT_PLACEMENT_KEY,
      JSON.stringify({ x: 400, y: 200, width: 380, height: 560, collapsed: false }),
    )
    await renderDock()
    const dialog = await screen.findByRole('dialog')

    // Drag the west edge far past the minimum width. The east edge is what the
    // user is holding still, so x must stop once width bottoms out rather than
    // marching right and dragging the whole panel with it.
    const handle = screen.getByTestId('assistant-dock-resize-w')
    fireEvent.pointerDown(handle, { clientX: 400, clientY: 500, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 2000, clientY: 500, pointerId: 1 })
    fireEvent.pointerUp(window, { clientX: 2000, clientY: 500, pointerId: 1 })

    expect(parseInt(dialog.style.width, 10)).toBe(MIN_DOCK_WIDTH)
    // Right edge stays where it started: 400 + 380.
    const right = parseInt(dialog.style.left, 10) + parseInt(dialog.style.width, 10)
    expect(right).toBe(780)
  })

  it('moves with a pointer drag on the title bar', async () => {
    const user = userEvent.setup()
    await renderDock()
    await user.click(await screen.findByRole('button', { name: /assistant/i }))

    const handle = await screen.findByTestId('assistant-dock-drag-handle')
    const dialog = screen.getByRole('dialog')
    const before = dialog.style.left

    fireEvent.pointerDown(handle, { clientX: 500, clientY: 500, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 400, clientY: 460, pointerId: 1 })
    fireEvent.pointerUp(window, { clientX: 400, clientY: 460, pointerId: 1 })

    await waitFor(() => expect(dialog.style.left).not.toBe(before))
  })
})
