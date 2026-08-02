import { Suspense, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Outlet } from 'react-router-dom'

import { AssistantDock } from '@/components/assistant/AssistantDock'
import { AppSidebar } from '@/components/layout/AppSidebar'
import { RouteFallback } from '@/components/layout/RouteFallback'
import { useSystemSettings } from '@/hooks/useSystemSettings'
import {
  TerminalRuntimeProvider,
} from '@/terminal/TerminalRuntimeProvider'
import { TerminalDock } from '@/terminal/TerminalDock'
import type { TerminalTransport } from '@/terminal/transport'

export interface AppLayoutProps {
  terminalTransport?: TerminalTransport
}

type TextField = HTMLInputElement | HTMLTextAreaElement

type TextMenuState = {
  field: TextField
  x: number
  y: number
  start: number
  end: number
  direction: 'forward' | 'backward' | 'none'
}

const TEXT_INPUT_TYPES = new Set(['email', 'password', 'search', 'tel', 'text', 'url'])

function textFieldFromTarget(target: EventTarget | null): TextField | null {
  if (!(target instanceof Element)) return null
  const field = target.closest('input, textarea')
  if (field instanceof HTMLTextAreaElement) return field
  return field instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(field.type) ? field : null
}

function replaceSelection(menu: TextMenuState, text: string, inputType: string) {
  const { field, start, end } = menu
  const value = `${field.value.slice(0, start)}${text}${field.value.slice(end)}`
  const prototype = field instanceof HTMLTextAreaElement
    ? HTMLTextAreaElement.prototype
    : HTMLInputElement.prototype
  Object.getOwnPropertyDescriptor(prototype, 'value')?.set?.call(field, value)
  const cursor = start + text.length
  field.setSelectionRange(cursor, cursor)
  field.dispatchEvent(new InputEvent('input', {
    bubbles: true,
    data: text || null,
    inputType,
  }))
}

export function AppLayout({ terminalTransport }: AppLayoutProps = {}) {
  const { t } = useTranslation('common')
  const systemSettings = useSystemSettings()
  const [textMenu, setTextMenu] = useState<TextMenuState | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!textMenu) return
    const closeOutside = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setTextMenu(null)
    }
    const close = () => setTextMenu(null)
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setTextMenu(null)
      textMenu.field.focus()
    }
    document.addEventListener('pointerdown', closeOutside)
    document.addEventListener('keydown', closeOnEscape)
    window.addEventListener('blur', close)
    window.addEventListener('resize', close)
    return () => {
      document.removeEventListener('pointerdown', closeOutside)
      document.removeEventListener('keydown', closeOnEscape)
      window.removeEventListener('blur', close)
      window.removeEventListener('resize', close)
    }
  }, [textMenu])

  const restoreSelection = (menu: TextMenuState) => {
    menu.field.focus()
    menu.field.setSelectionRange(menu.start, menu.end, menu.direction)
  }

  const runTextAction = async (
    action: 'cut' | 'copy' | 'paste' | 'pastePlain' | 'selectAll',
  ) => {
    const menu = textMenu
    if (!menu || !menu.field.isConnected) return
    setTextMenu(null)
    restoreSelection(menu)

    if (action === 'selectAll') {
      menu.field.select()
      return
    }
    if (action === 'paste' || action === 'pastePlain') {
      try {
        const text = await navigator.clipboard.readText()
        replaceSelection(menu, text, 'insertFromPaste')
      } catch {
        if (action === 'paste') document.execCommand('paste')
      }
      return
    }

    try {
      await navigator.clipboard.writeText(menu.field.value.slice(menu.start, menu.end))
      if (action === 'cut') replaceSelection(menu, '', 'deleteByCut')
    } catch {
      document.execCommand(action)
    }
  }

  return (
    <TerminalRuntimeProvider transport={terminalTransport}>
      <div
        className="flex h-full min-h-0 overflow-hidden bg-background"
        onContextMenu={(event) => {
          event.preventDefault()
          const field = textFieldFromTarget(event.target)
          if (!field) {
            setTextMenu(null)
            return
          }
          const start = field.selectionStart ?? 0
          const end = field.selectionEnd ?? start
          const menuWidth = 232
          const menuHeight = 188
          setTextMenu({
            field,
            start,
            end,
            direction: field.selectionDirection ?? 'none',
            x: Math.max(8, Math.min(event.clientX, window.innerWidth - menuWidth - 8)),
            y: Math.max(8, Math.min(event.clientY, window.innerHeight - menuHeight - 8)),
          })
        }}
      >
        <AppSidebar />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-hidden">
            <Suspense fallback={<RouteFallback />}>
              <Outlet />
            </Suspense>
          </div>
          <TerminalDock />
        </main>
        {/* Portals to document.body, so it floats over the whole shell rather
            than being clipped by the main column's overflow. Only mounted
            inside the authenticated tree: the login and register routes render
            outside AppLayout. */}
        {!systemSettings.isLoading && systemSettings.data?.assistant_enabled !== false ? (
          <AssistantDock />
        ) : null}
        {textMenu ? (
          <div
            ref={menuRef}
            role="menu"
            aria-label={t('textMenu.label')}
            className="fixed z-[100] w-56 rounded-xl border border-border/70 bg-popover p-1.5 text-popover-foreground shadow-xl"
            style={{ left: textMenu.x, top: textMenu.y }}
          >
            {([
              ['cut', t('textMenu.cut'), 'Ctrl+X'],
              ['copy', t('textMenu.copy'), 'Ctrl+C'],
              ['paste', t('textMenu.paste'), 'Ctrl+V'],
              ['pastePlain', t('textMenu.pastePlain'), ''],
              ['selectAll', t('textMenu.selectAll'), 'Ctrl+A'],
            ] as const).map(([action, label, shortcut]) => {
              const needsSelection = action === 'cut' || action === 'copy'
              const changesValue = action === 'cut' || action === 'paste' || action === 'pastePlain'
              const disabled = (needsSelection && textMenu.start === textMenu.end)
                || (changesValue && (textMenu.field.disabled || textMenu.field.readOnly))
              return (
                <button
                  key={action}
                  type="button"
                  role="menuitem"
                  disabled={disabled}
                  className="flex h-9 w-full items-center justify-between rounded-lg px-3 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent disabled:pointer-events-none disabled:opacity-40"
                  onClick={() => void runTextAction(action)}
                >
                  <span>{label}</span>
                  {shortcut ? (
                    <span className="ml-8 text-xs text-muted-foreground">{shortcut}</span>
                  ) : null}
                </button>
              )
            })}
          </div>
        ) : null}
      </div>
    </TerminalRuntimeProvider>
  )
}
