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
  const { subscribeOutput, write, resize } = useTerminalRuntime()

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
    let frame: number | null = null
    let processingResize = false
    let desiredSize: { cols: number; rows: number } | null = null
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
          if (succeeded && !disposed) lastSuccessfulSize = size
        }
      } finally {
        processingResize = false
      }
    }

    const scheduleFit = () => {
      if (frame !== null || disposed) return
      frame = requestAnimationFrame(() => {
        frame = null
        if (disposed) return
        try {
          fit.fit()
        } catch {
          return
        }
        if (terminal.cols <= 0 || terminal.rows <= 0) return
        desiredSize = { cols: terminal.cols, rows: terminal.rows }
        void processResize()
      })
    }

    const input = terminal.onData((data) => {
      void write(tab.tabId, data).catch(() => undefined)
    })
    const unsubscribeOutput = subscribeOutput(tab.tabId, (bytes) => terminal.write(bytes))
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
      input.dispose()
      unsubscribeOutput()
      resizeObserver.disconnect()
      themeObserver.disconnect()
      if (frame !== null) cancelAnimationFrame(frame)
      fit.dispose()
      terminal.dispose()
    }
  }, [resize, subscribeOutput, tab.tabId, write])

  return (
    <div
      ref={containerRef}
      className="terminal-pane h-full min-h-0 w-full overflow-hidden"
      data-terminal-tab-id={tab.tabId}
      data-testid={`terminal-pane-${tab.tabId}`}
    />
  )
}
