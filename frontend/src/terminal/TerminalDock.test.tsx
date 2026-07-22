import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { TerminalRuntimeContextValue, TerminalRuntimeTab } from '@/terminal/TerminalRuntimeProvider'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) => options?.defaultValue ?? _key,
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

  it('keeps panes from every conversation mounted and exposes accessible controls', () => {
    const { rerender } = render(<TerminalDock />)
    expect(screen.getByTestId('mock-pane-tab-a')).toBeInTheDocument()
    expect(screen.getByTestId('mock-pane-tab-b')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Rename terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Restart terminal' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Collapse terminal panel' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Maximize terminal panel' })).toBeInTheDocument()
    expect(screen.getByRole('separator', { name: 'Resize terminal panel' })).toBeInTheDocument()

    runtime = { ...runtime, isMaximized: true }
    rerender(<TerminalDock />)
    expect(screen.getByRole('button', { name: 'Restore terminal panel' })).toBeInTheDocument()
    const separator = screen.getByRole('separator', { name: 'Resize terminal panel' })
    expect(Number(separator.getAttribute('aria-valuenow'))).toBeLessThanOrEqual(
      Number(separator.getAttribute('aria-valuemax')),
    )
    expect(separator.className).toContain('h-3')
    expect(separator.className).not.toContain('-translate-y')
  })

  it('keeps pane instances mounted but unfocusable when collapsed or no route is registered', () => {
    const { rerender } = render(<TerminalDock />)
    const foregroundPane = screen.getByTestId('mock-pane-tab-a')
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
  ] as const)('removes %s %s-state controls from the DOM without unmounting panes', (hiddenBy, state) => {
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
    const pane = screen.getByTestId(state === 'empty' ? 'mock-pane-tab-b' : 'mock-pane-tab-a')

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
    const input = screen.getByRole('textbox', { name: 'Terminal name' })
    fireEvent.change(input, { target: { value: 'Build shell' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(actions.renameTab).toHaveBeenCalledWith('tab-a', 'Build shell')

    fireEvent.doubleClick(screen.getByRole('tab', { name: 'PowerShell' }))
    expect(screen.getByRole('textbox', { name: 'Terminal name' })).toBeInTheDocument()
  })

  it('keeps an empty open dock after the last tab closes', () => {
    runtime = { ...runtime, allTabs: [], activeTabs: [], activeTabId: null }
    render(<TerminalDock />)
    expect(screen.getByText('No terminals are open.')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'New terminal' })).not.toHaveLength(0)
  })

  it('persists dismissal of the one-time full-access warning', () => {
    const { unmount } = render(<TerminalDock />)
    expect(screen.getByText(/full host shell/i)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'I understand' }))
    expect(localStorage.getItem(FULL_ACCESS_WARNING_KEY)).toBe('dismissed')
    unmount()
    render(<TerminalDock />)
    expect(screen.queryByText(/full host shell/i)).not.toBeInTheDocument()
  })

  it.each([
    ['desktopRequired', 'Open the desktop app'],
    ['workspaceRequired', 'Bind a workspace'],
    ['localWorkspaceRequired', 'cloud workspaces are not supported'],
    ['pathRequired', 'valid local path'],
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
    expect(screen.getByRole('button', { name: 'New terminal' })).toBeDisabled()
  })

  it('shows exit and error recovery actions', () => {
    runtime = {
      ...runtime,
      allTabs: [{ ...firstTab, status: 'exited', exitCode: 7 }],
      activeTabs: [{ ...firstTab, status: 'exited', exitCode: 7 }],
    }
    const { rerender } = render(<TerminalDock />)
    expect(screen.getByText('Process exited with code 7')).toBeInTheDocument()

    const error = Object.assign(new Error('Unable to start PowerShell'), { code: 'terminal.spawn_failed' })
    runtime = {
      ...runtime,
      allTabs: [{ ...firstTab, status: 'error', error }],
      activeTabs: [{ ...firstTab, status: 'error', error }],
    }
    rerender(<TerminalDock />)
    expect(screen.getByText('Unable to start PowerShell')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
  })
})
