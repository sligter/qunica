import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { createMemoryRouter, RouterProvider } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AppLayout } from '@/components/layout/AppLayout'
import { TooltipProvider } from '@/components/ui/tooltip'
import { enUS } from '@/i18n/resources/en-US'
import { zhCN } from '@/i18n/resources/zh-CN'
import { useAuthStore } from '@/stores/authStore'
import type { TerminalTransport } from '@/terminal/transport'

vi.mock('@/components/assistant/AssistantDock', () => ({
  AssistantDock: () => <div data-testid="assistant-dock" />,
}))

// `appChildren` replaces the old hand-rolled `<Route>` children, so provide the
// context-menu test content on the eager home route rather than a lazy settings
// page — the suite needs it synchronously, and lazy would suspend the first
// render behind a Suspense boundary.
vi.mock('@/pages/home/ChatHomePage', () => ({
  ChatHomePage: () => (
    <>
      <div>Settings content</div>
      <input aria-label="Settings input" />
      <article data-copy-text="Full agent reply">Full agent reply</article>
      <span
        data-chat-agent-id="agent-1"
        data-chat-agent-name="Researcher"
        data-chat-conversation-id="group-1"
      >
        Researcher avatar
      </span>
      <textarea aria-label="Chat composer" data-chat-composer="group-1" defaultValue="Plan" />
    </>
  ),
}))

vi.mock('@/pages/agents/AgentsIndexPage', () => ({
  AgentsIndexPage: () => <div>Agents content</div>,
}))

vi.mock('@/pages/agents/AgentDetailPage', () => ({
  AgentDetailPage: () => <div>Agent details</div>,
}))

vi.mock('@/hooks/useAgents', () => ({
  useAgents: () => ({ data: [], isLoading: false, error: null }),
}))

function createFakeTransport(): TerminalTransport {
  return {
    create: vi.fn(),
    write: vi.fn(),
    resize: vi.fn(),
    close: vi.fn(),
    closeAll: vi.fn(),
  }
}

async function renderAppLayout(
  language: 'en-US' | 'zh-CN' = 'en-US',
  terminalTransport?: TerminalTransport,
  queryClient = new QueryClient(),
) {
  const i18n = i18next.createInstance()
  await i18n.use(initReactI18next).init({
    lng: language,
    fallbackLng: 'en-US',
    resources: { 'en-US': enUS, 'zh-CN': zhCN },
    interpolation: { escapeValue: false },
  })

  // The router only has to get `AppLayout` onto the screen — it re-matches the
  // real surface tree itself through `useRoutes(appChildren, …)`. Keeping the
  // data router's own tree to a single splat stops it from flattening (and
  // mutating) the shared route array across the many routers this file builds.
  const router = createMemoryRouter(
    [{ path: '*', element: <AppLayout terminalTransport={terminalTransport} /> }],
    { initialEntries: ['/'] },
  )

  const view = render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <RouterProvider router={router} />
        </TooltipProvider>
      </QueryClientProvider>
    </I18nextProvider>,
  )
  return { ...view, router }
}

describe('AppLayout', () => {
  it('uses a mobile drawer, preserves native long press, and closes the drawer on Back', async () => {
    vi.stubGlobal('matchMedia', vi.fn((media: string) => ({
      media, matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn(),
    })))
    try {
      const transport = createFakeTransport()
      const { router } = await renderAppLayout('en-US', transport)
      expect(screen.queryByTestId('terminal-dock-host')).not.toBeInTheDocument()
      const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
      fireEvent(screen.getByText('Full agent reply'), event)
      expect(event.defaultPrevented).toBe(false)
      fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))
      expect(await screen.findByRole('dialog', { name: 'Open navigation' })).toBeInTheDocument()
      await act(async () => { await router.navigate(-1) })
      expect(screen.queryByRole('dialog', { name: 'Open navigation' })).not.toBeInTheDocument()
      expect(transport.create).not.toHaveBeenCalled()
      expect(transport.closeAll).not.toHaveBeenCalled()
    } finally { vi.unstubAllGlobals() }
  })

  afterEach(() => {
    cleanup()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    vi.restoreAllMocks()
  })

  it('uses the compact text editing menu in inputs while suppressing the rest of the app', async () => {
    const { container } = await renderAppLayout()

    const surfaceEvent = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    expect(container.firstElementChild?.dispatchEvent(surfaceEvent)).toBe(false)
    expect(surfaceEvent.defaultPrevented).toBe(true)

    const input = screen.getByRole('textbox', { name: 'Settings input' }) as HTMLInputElement
    input.value = 'hello'
    input.setSelectionRange(1, 3)
    const inputEvent = new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: 40,
      clientY: 50,
    })
    expect(input.dispatchEvent(inputEvent)).toBe(false)
    expect(inputEvent.defaultPrevented).toBe(true)

    const menu = await screen.findByRole('menu', { name: 'Text editing menu' })
    expect(menu).toHaveStyle({ left: '40px', top: '50px' })
    expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      'CutCtrl+X',
      'CopyCtrl+C',
      'PasteCtrl+V',
      'Paste as plain text',
      'Select allCtrl+A',
    ])

    fireEvent.click(screen.getByRole('menuitem', { name: /Select all/ }))
    expect(input.selectionStart).toBe(0)
    expect(input.selectionEnd).toBe(input.value.length)
  })

  it('offers copy actions on content that declares its own text', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    // jsdom ships no clipboard, so define one for the duration of the test.
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    await renderAppLayout()

    const message = screen.getByText('Full agent reply')
    fireEvent.contextMenu(message, { clientX: 20, clientY: 30 })

    const menu = await screen.findByRole('menu', { name: 'Copy menu' })
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      'CopyCtrl+C',
      'Copy whole message',
      'Select message text',
    ])

    // Copying takes the source the message published, not whatever the
    // renderer laid out.
    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy whole message' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('Full agent reply'))
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()

    fireEvent.contextMenu(message, { clientX: 20, clientY: 30 })
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Select message text' }))
    const selection = window.getSelection()
    expect(selection?.rangeCount).toBe(1)
    expect(selection?.getRangeAt(0).commonAncestorContainer).toBe(message)

    // Right-clicking the selection copies the excerpt rather than the message.
    fireEvent.contextMenu(message, { clientX: 20, clientY: 30 })
    expect(await screen.findByRole('menuitem', { name: /Copy selection/ })).toBeVisible()
  })

  it('opens chat agent actions, inserts a mention, and links to the agent details', async () => {
    const { router } = await renderAppLayout()
    const avatar = screen.getByText('Researcher avatar')
    const composer = screen.getByRole('textbox', { name: 'Chat composer' })

    fireEvent.contextMenu(avatar, { clientX: 20, clientY: 30 })
    const menu = await screen.findByRole('menu', { name: 'Actions for Researcher' })
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      'View agent details',
      'Mention @Researcher',
    ])

    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Mention @Researcher' }))
    expect(composer).toHaveValue('Plan @Researcher ')
    expect(composer).toHaveFocus()

    fireEvent.contextMenu(avatar, { clientX: 20, clientY: 30 })
    fireEvent.click(await screen.findByRole('menuitem', { name: 'View agent details' }))
    await waitFor(() => expect(router.state.location.pathname).toBe('/agents/agent-1'))
  })

  it('ignores a selection left behind somewhere else', async () => {
    await renderAppLayout()
    const elsewhere = screen.getByText('Settings content')
    const range = document.createRange()
    range.selectNodeContents(elsewhere)
    window.getSelection()?.removeAllRanges()
    window.getSelection()?.addRange(range)

    fireEvent.contextMenu(screen.getByText('Full agent reply'), { clientX: 20, clientY: 30 })

    // The click landed outside the highlight, so "copy" means this message.
    const menu = await screen.findByRole('menu', { name: 'Copy menu' })
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      'CopyCtrl+C',
      'Copy whole message',
      'Select message text',
    ])
  })

  it('shows no menu where there is nothing to copy', async () => {
    await renderAppLayout()

    fireEvent.contextMenu(screen.getByText('Settings content'), { clientX: 5, clientY: 5 })

    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })

  it('clips the app shell so nothing can grow the document', async () => {
    const { container } = await renderAppLayout()

    // The shell fills its host and clips: any surface that overflows scrolls in
    // its own container rather than turning the document into a scroller.
    expect(container.firstElementChild).toHaveClass('h-full', 'min-h-0', 'overflow-hidden')
  })

  it('does not mount the assistant dock when it is disabled', async () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(['settings', 'system'], { assistant_enabled: false })

    await renderAppLayout('en-US', undefined, queryClient)

    expect(screen.queryByTestId('assistant-dock')).not.toBeInTheDocument()
  })

  it('renders English navigation labels', async () => {
    await renderAppLayout('en-US')

    const newChat = screen.getByRole('button', { name: 'New chat' })
    const newGroup = screen.getByRole('button', { name: 'New group' })
    expect(newChat.className).toContain('justify-center')
    expect(newGroup.className).toContain('justify-center')
    const directChats = screen.getByText('Chats')
    const groups = screen.getByText('Groups')
    expect(
      Boolean(directChats.compareDocumentPosition(groups) & Node.DOCUMENT_POSITION_FOLLOWING),
    ).toBe(true)
    expect(screen.getByText('Groups')).toBeInTheDocument()
    expect(screen.getByText('Library')).toBeInTheDocument()
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders Chinese navigation labels', async () => {
    await renderAppLayout('zh-CN')

    expect(screen.getByText('私聊')).toBeInTheDocument()
    expect(screen.getByText('群组')).toBeInTheDocument()
    expect(screen.getByText('资源库')).toBeInTheDocument()
    expect(screen.getByText('设置')).toBeInTheDocument()
  })

  it('keeps one terminal host mounted across routes without creating on non-chat routes', async () => {
    const transport = createFakeTransport()
    const { router } = await renderAppLayout('en-US', transport)
    const host = screen.getByTestId('terminal-dock-host')

    await act(async () => {
      await router.navigate('/agents')
    })

    expect(await screen.findByText('Agents content')).toBeInTheDocument()
    expect(screen.getByTestId('terminal-dock-host')).toBe(host)
    expect(transport.create).not.toHaveBeenCalled()
    expect(transport.close).not.toHaveBeenCalled()
  })

  it('waits for terminal cleanup before clearing auth and query state on logout', async () => {
    let releaseCleanup!: () => void
    const firstCleanup = new Promise<void>((resolve) => { releaseCleanup = resolve })
    const transport = createFakeTransport()
    vi.mocked(transport.closeAll)
      .mockImplementationOnce(() => firstCleanup)
      .mockResolvedValue(undefined)
    const queryClient = new QueryClient()
    const clear = vi.spyOn(queryClient, 'clear')
    useAuthStore.setState({
      user: {
        id: 'user-1', email: 'user@example.com', name: 'User One',
        avatar_url: null, created_at: '2026-01-01T00:00:00Z',
      },
    })
    await renderAppLayout('en-US', transport, queryClient)

    fireEvent.click(screen.getByRole('button', { name: 'User menu' }))
    const logout = screen.getByRole('button', { name: 'Log out' })
    fireEvent.click(logout)
    fireEvent.click(logout)

    await waitFor(() => expect(transport.closeAll).toHaveBeenCalledTimes(1))
    expect(useAuthStore.getState().user).not.toBeNull()
    expect(clear).not.toHaveBeenCalled()

    await act(async () => { releaseCleanup() })
    await waitFor(() => expect(useAuthStore.getState().user).toBeNull())
    expect(transport.closeAll).toHaveBeenCalledTimes(2)
    expect(clear).toHaveBeenCalledTimes(1)
  })

  it('continues logout after cleanup fails and logs only stable diagnostics', async () => {
    const transport = createFakeTransport()
    vi.mocked(transport.closeAll).mockRejectedValue(
      { code: 'terminal.cleanup_timeout', message: 'Cleanup timed out' },
    )
    const queryClient = new QueryClient()
    const clear = vi.spyOn(queryClient, 'clear')
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    useAuthStore.setState({
      user: {
        id: 'user-1', email: 'user@example.com', name: 'User One',
        avatar_url: null, created_at: '2026-01-01T00:00:00Z',
      },
    })
    await renderAppLayout('en-US', transport, queryClient)

    fireEvent.click(screen.getByRole('button', { name: 'User menu' }))
    fireEvent.click(screen.getByRole('button', { name: 'Log out' }))

    await waitFor(() => expect(useAuthStore.getState().user).toBeNull())
    expect(transport.closeAll).toHaveBeenCalledTimes(2)
    expect(clear).toHaveBeenCalledTimes(1)
    expect(consoleError).toHaveBeenCalledWith('[terminal] cleanup failed', {
      code: 'terminal.cleanup_timeout',
      message: 'Cleanup timed out',
    })
  })

  it('omits the conversation chrome inside an auxiliary desktop window', async () => {
    vi.stubGlobal('__TAURI_INTERNALS__', {
      metadata: { currentWindow: { label: 'library' } },
    })
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...window.location, hostname: 'tauri.localhost' },
    })

    const { router } = await renderAppLayout()
    await act(async () => {
      await router.navigate('/agents')
    })

    expect(await screen.findByText('Agents content')).toBeInTheDocument()
    expect(screen.queryByText('Library')).not.toBeInTheDocument()
    expect(screen.queryByText('Chats')).not.toBeInTheDocument()
    expect(screen.queryByTestId('assistant-dock')).not.toBeInTheDocument()
    expect(screen.queryByTestId('terminal-dock-host')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close window' })).toBeInTheDocument()
    vi.unstubAllGlobals()
  })
})
