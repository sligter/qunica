import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { TerminalRuntimeContextValue, TerminalRuntimeTab } from '@/terminal/TerminalRuntimeProvider'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      const translations: Record<string, string> = {
        'terminal.title': 'Terminal', 'terminal.new': 'New terminal',
        'terminal.rename': 'Rename terminal', 'terminal.close': 'Close terminal',
        'terminal.restart': 'Restart terminal', 'terminal.collapse': 'Collapse terminal',
        'terminal.maximize': 'Maximize terminal', 'terminal.restore': 'Restore terminal',
        'terminal.resize': 'Resize terminal height', 'terminal.empty': 'No terminal tabs.',
        'terminal.loading': 'Preparing terminal availability…', 'terminal.starting': 'Starting',
        'terminal.exited': 'Exited', 'terminal.exitCode': 'Exit code {{code}}',
        'terminal.retry': 'Retry', 'terminal.fullAccessTitle': 'Full local shell access',
        'terminal.fullAccessBody': 'This terminal starts in the workspace but can access other files and processes allowed by your operating-system account.',
        'terminal.dismiss': 'I understand',
        'terminal.desktopRequired': 'Terminal is available only in the desktop app.',
        'terminal.workspaceRequired': 'Bind a workspace to use the terminal.',
        'terminal.localWorkspaceRequired': 'Cloud sandbox terminals are not supported yet.',
        'terminal.pathRequired': 'The local workspace needs an absolute directory.',
        'terminal.spawnError': 'Unable to start the terminal: {{message}}',
      }
      return (translations[key] ?? key).replace(/{{(\w+)}}/g, (_, name: string) => String(options?.[name] ?? ''))
    },
  }),
}))

vi.mock('@/terminal/TerminalPane', () => ({
  TerminalPane: ({ tab }: { tab: TerminalRuntimeTab }) => <div data-testid={`mock-pane-${tab.tabId}`} />,
}))

const actions = {
  registerConversation: vi.fn(),
  toggleDock: vi.fn(async () => undefined),
  createTab: vi.fn(async () => undefined),
  selectTab: vi.fn(),
  renameTab: vi.fn(),
  closeTab: vi.fn(async () => undefined),
  restartTab: vi.fn(async () => undefined),
  closeConversation: vi.fn(async () => undefined),
  closeAll: vi.fn(async () => undefined),
  toggleMaximized: vi.fn(),
  setPaneHeight: vi.fn(),
  subscribeOutput: vi.fn(() => vi.fn()),
  write: vi.fn(async () => undefined),
  resize: vi.fn(async () => true),
}

const firstTab: TerminalRuntimeTab = {
  tabId: 'tab-a', conversationId: 'chat-a', sessionId: 'session-a', label: 'PowerShell',
  launchDirectory: 'D:/a', status: 'running', exitCode: null, error: null,
}
const backgroundTab: TerminalRuntimeTab = {
  tabId: 'tab-b', conversationId: 'chat-b', sessionId: 'session-b', label: 'bash',
  launchDirectory: '/work', status: 'running', exitCode: null, error: null,
}

let runtime: TerminalRuntimeContextValue

vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => runtime,
}))

import { FULL_ACCESS_WARNING_KEY, TerminalDock } from '@/terminal/TerminalDock'

function resetRuntime(): void {
  runtime = {
    activeConversation: { conversationId: 'chat-a', availability: 'ready', cwd: 'D:/a' },
    allTabs: [firstTab, backgroundTab],
    activeTabs: [firstTab],
    activeTabId: firstTab.tabId,
    isDockOpen: true,
    isMaximized: false,
    paneHeight: 350,
    ...actions,
  }
}

describe('TerminalDock', () => {
  beforeEach(() => {
    localStorage.clear()
    resetRuntime()
    vi.clearAllMocks()
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      disconnect() {}
    })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('keeps panes from every conversation mounted and exposes accessible controls', async () => {
    const { rerender } = render(<TerminalDock />)
    // Panes load lazily with the xterm runtime; await the first resolution.
    expect(await screen.findByTestId('mock-pane-tab-a')).toBeInTheDocument()
    expect(screen.getByTestId('mock-pane-tab-b')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Rename terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Restart terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Collapse terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Maximize terminal' })).toBeInTheDocument()
    expect(screen.getByRole('separator', { name: 'Resize terminal height' })).toBeInTheDocument()

    runtime = { ...runtime, isMaximized: true }
    rerender(<TerminalDock />)
    expect(screen.getByRole('button', { name: 'Restore terminal' })).toBeInTheDocument()
    const separator = screen.getByRole('separator', { name: 'Resize terminal height' })
    expect(Number(separator.getAttribute('aria-valuenow'))).toBeLessThanOrEqual(
      Number(separator.getAttribute('aria-valuemax')),
    )
    expect(separator.className).toContain('h-3')
    expect(separator.className).not.toContain('-translate-y')
  })

  it('keeps pane instances mounted but unfocusable when collapsed or no route is registered', async () => {
    const { rerender } = render(<TerminalDock />)
    const foregroundPane = await screen.findByTestId('mock-pane-tab-a')
    const backgroundPane = screen.getByTestId('mock-pane-tab-b')
    expect(foregroundPane.parentElement).not.toHaveAttribute('hidden')
    expect(backgroundPane.parentElement).toHaveAttribute('hidden')

    runtime = { ...runtime, isDockOpen: false }
    rerender(<TerminalDock />)
    expect(screen.getByTestId('mock-pane-tab-a')).toBe(foregroundPane)
    expect(screen.getByTestId('mock-pane-tab-b')).toBe(backgroundPane)
    expect(foregroundPane.parentElement).toHaveAttribute('hidden')

    runtime = { ...runtime, activeConversation: null }
    rerender(<TerminalDock />)
    expect(screen.getByTestId('mock-pane-tab-a')).toBe(foregroundPane)
    expect(foregroundPane.parentElement).toHaveAttribute('hidden')
    expect(screen.getByTestId('terminal-dock-host')).toHaveAttribute('aria-hidden', 'true')
  })

  it.each([
    ['collapsed', 'empty'],
    ['collapsed', 'exited'],
    ['collapsed', 'error'],
    ['route hidden', 'empty'],
    ['route hidden', 'exited'],
    ['route hidden', 'error'],
  ] as const)('removes %s %s-state controls from the DOM without unmounting panes', async (hiddenBy, state) => {
    const error = Object.assign(new Error('Unable to start PowerShell'), { code: 'terminal.spawn_failed' })
    const activeStateTab = state === 'exited'
      ? { ...firstTab, status: 'exited' as const, exitCode: 7 }
      : { ...firstTab, status: 'error' as const, error }

    runtime = state === 'empty'
      ? { ...runtime, allTabs: [backgroundTab], activeTabs: [], activeTabId: null }
      : {
          ...runtime,
          allTabs: [activeStateTab, backgroundTab],
          activeTabs: [activeStateTab],
          activeTabId: activeStateTab.tabId,
        }

    const { rerender } = render(<TerminalDock />)
    const pane = await screen.findByTestId(state === 'empty' ? 'mock-pane-tab-b' : 'mock-pane-tab-a')

    if (state === 'empty') {
      expect(screen.getAllByRole('button', { name: 'New terminal' }).length).toBeGreaterThan(0)
    } else if (state === 'exited') {
      expect(screen.getAllByRole('button', { name: 'Restart terminal' }).length).toBeGreaterThan(0)
    } else {
      expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
    }

    runtime = hiddenBy === 'collapsed'
      ? { ...runtime, isDockOpen: false }
      : { ...runtime, activeConversation: null }
    rerender(<TerminalDock />)

    const hiddenControlName = state === 'empty'
      ? 'New terminal'
      : state === 'exited'
        ? 'Restart terminal'
        : 'Retry'
    expect(screen.queryAllByRole('button', { name: hiddenControlName, hidden: true })).toHaveLength(0)
    expect(screen.getByTestId(state === 'empty' ? 'mock-pane-tab-b' : 'mock-pane-tab-a')).toBe(pane)
    expect(screen.getByTestId('terminal-dock-host')).toHaveAttribute('aria-hidden', 'true')
  })

  it('supports inline rename from the toolbar and tab double click', () => {
    render(<TerminalDock />)
    fireEvent.click(screen.getByRole('button', { name: 'Rename terminal' }))
    const input = screen.getByRole('textbox', { name: 'Rename terminal' })
    fireEvent.change(input, { target: { value: 'Build shell' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(actions.renameTab).toHaveBeenCalledWith('tab-a', 'Build shell')

    fireEvent.doubleClick(screen.getByRole('tab', { name: 'PowerShell' }))
    expect(screen.getByRole('textbox', { name: 'Rename terminal' })).toBeInTheDocument()
  })

  it('keeps an empty open dock after the last tab closes', () => {
    runtime = { ...runtime, allTabs: [], activeTabs: [], activeTabId: null }
    render(<TerminalDock />)
    expect(screen.getByText('No terminal tabs.')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'New terminal' })).not.toHaveLength(0)
  })

  it('persists dismissal of the one-time full-access warning', () => {
    const { unmount } = render(<TerminalDock />)
    expect(screen.getByText(/full local shell access/i)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'I understand' }))
    expect(localStorage.getItem(FULL_ACCESS_WARNING_KEY)).toBe('dismissed')
    unmount()
    render(<TerminalDock />)
    expect(screen.queryByText(/full local shell access/i)).not.toBeInTheDocument()
  })

  it.each([
    ['desktopRequired', 'desktop app'],
    ['workspaceRequired', 'Bind a workspace'],
    ['localWorkspaceRequired', 'Cloud sandbox terminals are not supported'],
    ['pathRequired', 'absolute directory'],
  ] as const)('shows the %s unavailable state', (availability, message) => {
    runtime = {
      ...runtime,
      activeConversation: { conversationId: 'chat-a', availability },
      allTabs: [],
      activeTabs: [],
      activeTabId: null,
    }
    render(<TerminalDock />)
    expect(screen.getByText(new RegExp(message, 'i'))).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'New terminal' })).not.toBeInTheDocument()
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument()
  })

  it.each([
    ['desktopRequired', 'starting', 'desktop app', /^Starting$/i],
    ['workspaceRequired', 'exited', 'Bind a workspace', /Process exited/i],
    ['pathRequired', 'error', 'absolute directory', /Unable to start the terminal/i],
  ] as const)(
    'prioritizes the %s unavailable state over an existing %s tab',
    async (availability, status, unavailableText, hiddenStatusText) => {
      const error = Object.assign(new Error('Unable to start PowerShell'), { code: 'terminal.spawn_failed' })
      const stateTab: TerminalRuntimeTab = {
        ...firstTab,
        status,
        exitCode: status === 'exited' ? 7 : null,
        error: status === 'error' ? error : null,
      }
      runtime = {
        ...runtime,
        allTabs: [stateTab, backgroundTab],
        activeTabs: [stateTab],
        activeTabId: stateTab.tabId,
      }

      const { rerender } = render(<TerminalDock />)
      const pane = await screen.findByTestId('mock-pane-tab-a')
      expect(pane.parentElement).not.toHaveAttribute('hidden')

      runtime = {
        ...runtime,
        activeConversation: { conversationId: 'chat-a', availability },
      }
      rerender(<TerminalDock />)

      expect(screen.getByText(new RegExp(unavailableText, 'i'))).toBeInTheDocument()
      expect(screen.queryByText(hiddenStatusText)).not.toBeInTheDocument()
      expect(screen.queryByRole('tablist')).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'New terminal' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Rename terminal' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Close terminal' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Restart terminal' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument()
      expect(screen.getByTestId('mock-pane-tab-a')).toBe(pane)
      expect(pane.parentElement).toHaveAttribute('hidden')
    },
  )

  it('shows exit and error recovery actions', () => {
    runtime = {
      ...runtime,
      allTabs: [{ ...firstTab, status: 'exited', exitCode: 7 }],
      activeTabs: [{ ...firstTab, status: 'exited', exitCode: 7 }],
    }
    const { rerender } = render(<TerminalDock />)
    expect(screen.getByText('Exit code 7')).toBeInTheDocument()

    const error = Object.assign(new Error('Unable to start PowerShell'), { code: 'terminal.spawn_failed' })
    runtime = {
      ...runtime,
      allTabs: [{ ...firstTab, status: 'error', error }],
      activeTabs: [{ ...firstTab, status: 'error', error }],
    }
    rerender(<TerminalDock />)
    expect(screen.getByText('Unable to start the terminal: Unable to start PowerShell')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
  })
})
