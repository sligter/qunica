import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import i18next from 'i18next'
import { I18nextProvider, initReactI18next } from 'react-i18next'
import { Link, MemoryRouter, Route, Routes } from 'react-router-dom'
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

  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <MemoryRouter initialEntries={['/settings']}>
            <Routes>
              <Route element={<AppLayout terminalTransport={terminalTransport} />}>
                <Route
                  path="settings"
                  element={<><div>Settings content</div><input aria-label="Settings input" /><Link to="/agents">Agents route</Link></>}
                />
                <Route path="agents" element={<div>Agents content</div>} />
              </Route>
            </Routes>
          </MemoryRouter>
        </TooltipProvider>
      </QueryClientProvider>
    </I18nextProvider>,
  )
}

describe('AppLayout', () => {
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
    expect(screen.getByText('Agents')).toBeInTheDocument()
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders Chinese navigation labels', async () => {
    await renderAppLayout('zh-CN')

    expect(screen.getByText('私聊')).toBeInTheDocument()
    expect(screen.getByText('群组')).toBeInTheDocument()
    expect(screen.getByText('Agent')).toBeInTheDocument()
    expect(screen.getByText('设置')).toBeInTheDocument()
  })

  it('keeps one terminal host mounted across routes without creating on non-chat routes', async () => {
    const transport = createFakeTransport()
    await renderAppLayout('en-US', transport)
    const host = screen.getByTestId('terminal-dock-host')

    fireEvent.click(screen.getByRole('link', { name: 'Agents route' }))

    expect(screen.getByText('Agents content')).toBeInTheDocument()
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
})
