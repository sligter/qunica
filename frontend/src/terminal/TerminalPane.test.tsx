import { act, cleanup, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { TerminalRuntimeTab } from '@/terminal/TerminalRuntimeProvider'

const mocks = vi.hoisted(() => {
  const terminals: MockTerminal[] = []
  const fitAddons: MockFitAddon[] = []
  class MockTerminal {
    options: Record<string, unknown>
    cols = 100
    rows = 30
    loadAddon = vi.fn()
    open = vi.fn()
    write = vi.fn()
    dispose = vi.fn()
    inputListener: ((data: string) => void) | null = null
    inputDisposable = { dispose: vi.fn() }
    constructor(options: Record<string, unknown>) {
      this.options = options
      terminals.push(this)
    }
    onData(listener: (data: string) => void) {
      this.inputListener = listener
      return this.inputDisposable
    }
  }
  class MockFitAddon {
    fit = vi.fn()
    dispose = vi.fn()
    constructor() {
      fitAddons.push(this)
    }
  }
  return { terminals, fitAddons, MockTerminal, MockFitAddon }
})

vi.mock('@xterm/xterm', () => ({ Terminal: mocks.MockTerminal }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: mocks.MockFitAddon }))

const runtime = vi.hoisted(() => ({
  subscribeOutput: vi.fn(),
  write: vi.fn(),
  resize: vi.fn(),
}))

vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => runtime,
}))

import { TerminalPane } from '@/terminal/TerminalPane'

const tab: TerminalRuntimeTab = {
  tabId: 'tab-a',
  conversationId: 'chat-a',
  sessionId: 'session-a',
  label: 'PowerShell',
  launchDirectory: 'D:/workspace',
  status: 'running',
  exitCode: null,
  error: null,
}

describe('TerminalPane', () => {
  let resizeCallback: ResizeObserverCallback
  let themeCallback: MutationCallback
  let outputListener: ((bytes: Uint8Array) => void) | null
  let frames: FrameRequestCallback[]
  const disconnectResize = vi.fn()
  const disconnectTheme = vi.fn()

  beforeEach(() => {
    mocks.terminals.length = 0
    mocks.fitAddons.length = 0
    outputListener = null
    frames = []
    runtime.write.mockReset().mockResolvedValue(undefined)
    runtime.resize.mockReset().mockResolvedValue(true)
    runtime.subscribeOutput.mockReset().mockImplementation((_, listener) => {
      outputListener = listener
      return vi.fn()
    })
    vi.stubGlobal('requestAnimationFrame', vi.fn((callback: FrameRequestCallback) => {
      frames.push(callback)
      return frames.length
    }))
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
    vi.stubGlobal('ResizeObserver', class {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback
      }
      observe() {}
      disconnect = disconnectResize
    })
    vi.stubGlobal('MutationObserver', class {
      constructor(callback: MutationCallback) {
        themeCallback = callback
      }
      observe() {}
      disconnect = disconnectTheme
    })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  it('constructs one configured xterm and connects input and output', async () => {
    render(<TerminalPane tab={tab} />)
    const terminal = mocks.terminals[0]!
    expect(mocks.terminals).toHaveLength(1)
    expect(terminal.options).toMatchObject({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      fontSize: 14,
      scrollback: 5000,
    })

    terminal.inputListener?.('pwd\r')
    expect(runtime.write).toHaveBeenCalledWith('tab-a', 'pwd\r')
    const bytes = new Uint8Array([0xe4, 0xb8, 0xad])
    outputListener?.(bytes)
    expect(terminal.write).not.toHaveBeenCalled()
    await act(async () => frames.pop()?.(0))
    expect(terminal.write).toHaveBeenCalledWith(bytes)

    act(() => themeCallback([], {} as MutationObserver))
    expect(terminal.options.theme).toBeDefined()
  })

  it('coalesces observations and retries the same size after a non-fatal failure', async () => {
    runtime.resize.mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    render(<TerminalPane tab={tab} />)
    expect(frames).toHaveLength(1)
    act(() => {
      resizeCallback([], {} as ResizeObserver)
      resizeCallback([], {} as ResizeObserver)
    })
    expect(frames).toHaveLength(1)

    await act(async () => frames.shift()?.(0))
    expect(mocks.fitAddons[0]?.fit).toHaveBeenCalledTimes(1)
    expect(runtime.resize).toHaveBeenCalledWith('tab-a', 100, 30)

    act(() => resizeCallback([], {} as ResizeObserver))
    await act(async () => frames.shift()?.(16))
    expect(runtime.resize).toHaveBeenCalledTimes(2)
  })

  it('keeps xterm alive and resends an unchanged size when the session changes', async () => {
    const { rerender } = render(<TerminalPane tab={tab} />)
    const terminal = mocks.terminals[0]!

    await act(async () => frames.shift()?.(0))
    expect(runtime.resize).toHaveBeenLastCalledWith('tab-a', 100, 30)

    rerender(<TerminalPane tab={{ ...tab, sessionId: 'session-b' }} />)
    expect(mocks.terminals).toHaveLength(1)
    expect(terminal.dispose).not.toHaveBeenCalled()
    await act(async () => frames.shift()?.(16))

    expect(runtime.resize).toHaveBeenCalledTimes(2)
    expect(runtime.resize).toHaveBeenLastCalledWith('tab-a', 100, 30)
  })

  it('ignores input until the session is running without recreating xterm', () => {
    const { rerender } = render(
      <TerminalPane tab={{ ...tab, sessionId: null, status: 'starting' }} />,
    )
    const terminal = mocks.terminals[0]!

    terminal.inputListener?.('too early')
    expect(runtime.write).not.toHaveBeenCalled()

    rerender(<TerminalPane tab={{ ...tab, sessionId: 'session-ready', status: 'running' }} />)
    terminal.inputListener?.('ready\r')
    expect(mocks.terminals).toHaveLength(1)
    expect(runtime.write).toHaveBeenCalledWith('tab-a', 'ready\r')
  })

  it('merges ordered output chunks once per frame and cancels pending output on dispose', async () => {
    const { rerender, unmount } = render(<TerminalPane tab={tab} />)
    const terminal = mocks.terminals[0]!

    outputListener?.(new Uint8Array([1, 2]))
    outputListener?.(new Uint8Array([3, 4]))
    expect(terminal.write).not.toHaveBeenCalled()
    expect(frames).toHaveLength(2)
    await act(async () => frames.pop()?.(0))
    expect(terminal.write).toHaveBeenCalledTimes(1)
    expect(terminal.write).toHaveBeenCalledWith(new Uint8Array([1, 2, 3, 4]))

    rerender(<TerminalPane tab={{ ...tab, sessionId: 'session-restarted' }} />)
    expect(runtime.subscribeOutput).toHaveBeenCalledTimes(1)
    outputListener?.(new Uint8Array([5]))
    const pendingOutput = frames.at(-1)
    unmount()
    pendingOutput?.(16)
    expect(terminal.write).toHaveBeenCalledTimes(1)
    expect(cancelAnimationFrame).toHaveBeenCalled()
  })

  it('disposes terminal resources only when the pane is removed', () => {
    const { rerender, unmount } = render(<TerminalPane tab={tab} />)
    const terminal = mocks.terminals[0]!
    rerender(<TerminalPane tab={{ ...tab, status: 'exited', exitCode: 0 }} />)
    expect(terminal.dispose).not.toHaveBeenCalled()
    unmount()
    expect(terminal.inputDisposable.dispose).toHaveBeenCalled()
    expect(mocks.fitAddons[0]?.dispose).toHaveBeenCalled()
    expect(terminal.dispose).toHaveBeenCalled()
    expect(disconnectResize).toHaveBeenCalled()
    expect(disconnectTheme).toHaveBeenCalled()
  })
})
