import { FitAddon } from '@xterm/addon-fit'
import { Terminal, type ITheme } from '@xterm/xterm'
import { useEffect, useRef } from 'react'

import '@xterm/xterm/css/xterm.css'

import {
  useTerminalRuntime,
  type TerminalRuntimeTab,
} from '@/terminal/TerminalRuntimeProvider'

const TERMINAL_FONT = '"Cascadia Code", "JetBrains Mono", "SFMono-Regular", Consolas, monospace'

function cssVariable(styles: CSSStyleDeclaration, name: string, fallback: string): string {
  return styles.getPropertyValue(name).trim() || fallback
}

// Exported for deterministic theme contract tests.
// eslint-disable-next-line react-refresh/only-export-components
export function terminalThemeFromDocument(): ITheme {
  const styles = getComputedStyle(document.documentElement)
  return {
    background: cssVariable(styles, '--terminal-background', '#171512'),
    foreground: cssVariable(styles, '--terminal-foreground', '#ece7df'),
    cursor: cssVariable(styles, '--terminal-cursor', '#d97955'),
    cursorAccent: cssVariable(styles, '--terminal-background', '#171512'),
    selectionBackground: cssVariable(styles, '--terminal-selection', '#51463d'),
  }
}

export interface TerminalPaneProps {
  tab: TerminalRuntimeTab
}

export function TerminalPane({ tab }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const sessionRef = useRef({ sessionId: tab.sessionId, status: tab.status })
  const resetResizeRef = useRef<(() => void) | null>(null)
  const { subscribeOutput, write, resize } = useTerminalRuntime()

  sessionRef.current = { sessionId: tab.sessionId, status: tab.status }

  useEffect(() => {
    const container = containerRef.current
    if (container === null) return

    const terminal = new Terminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      fontFamily: TERMINAL_FONT,
      fontSize: 13,
      scrollback: 5_000,
      theme: terminalThemeFromDocument(),
    })
    const fit = new FitAddon()
    terminal.loadAddon(fit)
    terminal.open(container)

    let disposed = false
    let fitFrame: number | null = null
    let outputFrame: number | null = null
    let outputChunks: Uint8Array[] = []
    let outputBytes = 0
    let processingResize = false
    let resizeSessionEpoch = 0
    let desiredSize: { cols: number; rows: number; epoch: number } | null = null
    let lastSuccessfulSize: { cols: number; rows: number } | null = null

    const processResize = async () => {
      if (processingResize || disposed) return
      processingResize = true
      try {
        while (!disposed && desiredSize !== null) {
          const size = desiredSize
          desiredSize = null
          if (
            lastSuccessfulSize?.cols === size.cols &&
            lastSuccessfulSize.rows === size.rows
          ) {
            continue
          }
          const succeeded = await resize(tab.tabId, size.cols, size.rows)
          if (succeeded && !disposed && size.epoch === resizeSessionEpoch) {
            lastSuccessfulSize = size
          }
        }
      } finally {
        processingResize = false
      }
    }

    const scheduleFit = () => {
      if (fitFrame !== null || disposed) return
      fitFrame = requestAnimationFrame(() => {
        fitFrame = null
        if (disposed) return
        try {
          fit.fit()
        } catch {
          return
        }
        if (terminal.cols <= 0 || terminal.rows <= 0) return
        desiredSize = {
          cols: terminal.cols,
          rows: terminal.rows,
          epoch: resizeSessionEpoch,
        }
        void processResize()
      })
    }

    const scheduleOutput = (bytes: Uint8Array) => {
      if (disposed) return
      const chunk = bytes.slice()
      outputChunks.push(chunk)
      outputBytes += chunk.byteLength
      if (outputFrame !== null) return
      outputFrame = requestAnimationFrame(() => {
        outputFrame = null
        if (disposed || outputChunks.length === 0) return
        const combined = new Uint8Array(outputBytes)
        let offset = 0
        for (const queued of outputChunks) {
          combined.set(queued, offset)
          offset += queued.byteLength
        }
        outputChunks = []
        outputBytes = 0
        terminal.write(combined)
      })
    }

    resetResizeRef.current = () => {
      resizeSessionEpoch += 1
      lastSuccessfulSize = null
      scheduleFit()
    }

    const input = terminal.onData((data) => {
      if (
        sessionRef.current.status !== 'running' ||
        sessionRef.current.sessionId === null
      ) {
        return
      }
      void write(tab.tabId, data).catch(() => undefined)
    })
    const unsubscribeOutput = subscribeOutput(tab.tabId, scheduleOutput)
    const resizeObserver = new ResizeObserver(scheduleFit)
    resizeObserver.observe(container)
    scheduleFit()

    const themeObserver = new MutationObserver(() => {
      terminal.options.theme = terminalThemeFromDocument()
    })
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class', 'data-theme', 'style'],
    })

    return () => {
      disposed = true
      resetResizeRef.current = null
      input.dispose()
      unsubscribeOutput()
      resizeObserver.disconnect()
      themeObserver.disconnect()
      if (fitFrame !== null) cancelAnimationFrame(fitFrame)
      if (outputFrame !== null) cancelAnimationFrame(outputFrame)
      outputChunks = []
      outputBytes = 0
      fit.dispose()
      terminal.dispose()
    }
  }, [resize, subscribeOutput, tab.tabId, write])

  useEffect(() => {
    resetResizeRef.current?.()
  }, [tab.sessionId])

  return (
    <div
      ref={containerRef}
      className="terminal-pane h-full min-h-0 w-full overflow-hidden"
      data-terminal-tab-id={tab.tabId}
      data-testid={`terminal-pane-${tab.tabId}`}
    />
  )
}
