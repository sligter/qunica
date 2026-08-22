import { Suspense, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate, useRoutes, type Location } from 'react-router-dom'

import { AssistantDock } from '@/components/assistant/AssistantDock'
import { AppSidebar } from '@/components/layout/AppSidebar'
import { CommandPaletteProvider } from '@/components/layout/CommandPalette'
import { RouteFallback } from '@/components/layout/RouteFallback'
import {
  OverlayProvider,
  isOverlayPath,
  overlayAreaLabelKey,
  type OverlayLocationState,
} from '@/components/layout/overlayRouting'
import { SettingsOverlay } from '@/components/layout/SettingsOverlay'
import { UnsavedChangesProvider } from '@/components/layout/UnsavedChangesProvider'
import { appChildren } from '@/routes/appRoutes'
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

/** Right-clicking a text field offers editing; anywhere else offers copying. */
type MenuState =
  | {
      kind: 'field'
      x: number
      y: number
      field: TextField
      start: number
      end: number
      direction: 'forward' | 'backward' | 'none'
    }
  | {
      kind: 'content'
      x: number
      y: number
      /** Nearest element that declared copyable text, when there is one. */
      node: HTMLElement | null
      text: string
      /** What was selected when the menu opened, captured before focus moves. */
      selection: string
    }

type FieldAction = 'cut' | 'copy' | 'paste' | 'pastePlain' | 'selectAll'
type ContentAction = 'copy' | 'copyAll' | 'selectAll'

const TEXT_INPUT_TYPES = new Set(['email', 'password', 'search', 'tel', 'text', 'url'])

const MENU_WIDTH = 232
const MENU_ITEM_HEIGHT = 36
const MENU_PADDING = 12

function textFieldFromTarget(target: EventTarget | null): TextField | null {
  if (!(target instanceof Element)) return null
  const field = target.closest('input, textarea')
  if (field instanceof HTMLTextAreaElement) return field
  return field instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(field.type) ? field : null
}

/**
 * The nearest ancestor that published its own plain-text source. Messages carry
 * it so "copy message" yields the markdown the agent wrote rather than whatever
 * the renderer happened to lay out.
 */
function copyableFromTarget(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null
  const node = target.closest('[data-copy-text]')
  return node instanceof HTMLElement ? node : null
}

/**
 * Highlighted text, but only when the click landed in it. A selection left
 * behind in another message must not quietly become what "copy" yields.
 */
function selectionAt(target: EventTarget | null): string {
  const selection = window.getSelection()
  const text = selection?.toString() ?? ''
  if (!text || !selection || selection.rangeCount === 0) return ''
  if (!(target instanceof Node)) return ''
  const container = selection.getRangeAt(0).commonAncestorContainer
  return container.contains(target) || target.contains(container) ? text : ''
}

function selectNodeContents(node: HTMLElement): void {
  const selection = window.getSelection()
  if (!selection) return
  const range = document.createRange()
  range.selectNodeContents(node)
  selection.removeAllRanges()
  selection.addRange(range)
}

async function writeClipboard(text: string): Promise<void> {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    document.execCommand('copy')
  }
}

function replaceSelection(menu: Extract<MenuState, { kind: 'field' }>, text: string, inputType: string) {
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
  const { t } = useTranslation(['common', 'navigation', 'groups'])
  const systemSettings = useSystemSettings()
  const [menu, setMenu] = useState<MenuState | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const navigate = useNavigate()
  const location = useLocation()

  // The settings overlay is React Router's background-location pattern: the
  // real browser location is the overlay URL, and the conversation it was
  // opened over rides along in `location.state.backgroundLocation`. When that
  // state is absent but the pathname is still an overlay area — an in-overlay
  // hop like "save → detail page", whose `navigate` did not carry the state —
  // fall back to the last conversation `AppLayout` remembered.
  const overlayState = location.state as OverlayLocationState | null
  const stageRef = useRef<Location | null>(null)
  const overlayOpen =
    isOverlayPath(location.pathname) &&
    (overlayState?.backgroundLocation != null || stageRef.current != null)
  const background: Location | null = overlayOpen
    ? (overlayState?.backgroundLocation ?? stageRef.current)
    : null
  const stage: Location = background ?? location

  useEffect(() => {
    // Remember the last real conversation/hub surface; overlay URLs are never
    // stored, so a cold deep link renders full-page instead of improvising a
    // stage out of itself.
    if (!isOverlayPath(location.pathname)) stageRef.current = location
  }, [location])

  const closeOverlay = () => {
    if (background) {
      void navigate({
        pathname: background.pathname,
        search: background.search,
        hash: background.hash,
      })
    } else {
      void navigate('/')
    }
  }

  const stageElement = useRoutes(appChildren, stage)
  const overlayElement = useRoutes(appChildren, location)
  const overlayLabel = overlayOpen ? t(overlayAreaLabelKey(location.pathname)) : undefined

  useEffect(() => {
    if (!menu) return
    const closeOutside = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenu(null)
    }
    const close = () => setMenu(null)
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setMenu(null)
      if (menu.kind === 'field') menu.field.focus()
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
  }, [menu])

  const restoreSelection = (state: Extract<MenuState, { kind: 'field' }>) => {
    state.field.focus()
    state.field.setSelectionRange(state.start, state.end, state.direction)
  }

  const runFieldAction = async (action: FieldAction) => {
    if (menu?.kind !== 'field' || !menu.field.isConnected) return
    setMenu(null)
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

  const runContentAction = async (action: ContentAction) => {
    if (menu?.kind !== 'content') return
    const { node, text, selection } = menu
    setMenu(null)

    if (action === 'selectAll') {
      if (node?.isConnected) selectNodeContents(node)
      return
    }
    // Plain "copy" honours a highlighted excerpt; "copy all" always takes the
    // whole message, which is why both are offered rather than one guessing.
    await writeClipboard(action === 'copy' ? selection || text : text)
  }

  const openMenu = (event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault()
    const clamp = (items: number) => ({
      x: Math.max(8, Math.min(event.clientX, window.innerWidth - MENU_WIDTH - 8)),
      y: Math.max(
        8,
        Math.min(event.clientY, window.innerHeight - (items * MENU_ITEM_HEIGHT + MENU_PADDING) - 8),
      ),
    })

    const field = textFieldFromTarget(event.target)
    if (field) {
      const start = field.selectionStart ?? 0
      const end = field.selectionEnd ?? start
      setMenu({
        kind: 'field',
        field,
        start,
        end,
        direction: field.selectionDirection ?? 'none',
        ...clamp(5),
      })
      return
    }

    // Read the selection before the menu takes focus — clicking the menu would
    // otherwise collapse it and copy nothing.
    const selection = selectionAt(event.target)
    const node = copyableFromTarget(event.target)
    const text = node?.dataset.copyText ?? ''
    if (!selection && !text) {
      setMenu(null)
      return
    }
    setMenu({ kind: 'content', node, text, selection, ...clamp(node ? 3 : 1) })
  }

  const menuItems: Array<{ key: string; label: string; shortcut: string; disabled: boolean; run: () => void }> =
    menu === null
      ? []
      : menu.kind === 'field'
        ? ([
            ['cut', t('textMenu.cut'), 'Ctrl+X'],
            ['copy', t('textMenu.copy'), 'Ctrl+C'],
            ['paste', t('textMenu.paste'), 'Ctrl+V'],
            ['pastePlain', t('textMenu.pastePlain'), ''],
            ['selectAll', t('textMenu.selectAll'), 'Ctrl+A'],
          ] as const).map(([action, label, shortcut]) => {
            const needsSelection = action === 'cut' || action === 'copy'
            const changesValue = action === 'cut' || action === 'paste' || action === 'pastePlain'
            return {
              key: action,
              label,
              shortcut,
              disabled:
                (needsSelection && menu.start === menu.end) ||
                (changesValue && (menu.field.disabled || menu.field.readOnly)),
              run: () => void runFieldAction(action),
            }
          })
        : [
            {
              key: 'copy',
              label: menu.selection ? t('textMenu.copySelection') : t('textMenu.copy'),
              shortcut: 'Ctrl+C',
              disabled: !menu.selection && !menu.text,
              run: () => void runContentAction('copy'),
            },
            ...(menu.node
              ? [
                  {
                    key: 'copyAll',
                    label: t('textMenu.copyMessage'),
                    shortcut: '',
                    disabled: !menu.text,
                    run: () => void runContentAction('copyAll'),
                  },
                  {
                    key: 'selectAll',
                    label: t('textMenu.selectMessage'),
                    shortcut: '',
                    disabled: false,
                    run: () => void runContentAction('selectAll'),
                  },
                ]
              : []),
          ]

  return (
    // Wrapping the unsaved-changes context rather than inside it: a language
    // switch re-renders this tree, and the palette must survive as a stable
    // element so its global chord listener is never torn down mid-typing.
    <CommandPaletteProvider>
    <UnsavedChangesProvider>
      <TerminalRuntimeProvider
        transport={terminalTransport}
        shell={systemSettings.data?.shell_preference}
      >
      <div
        className="flex h-full min-h-0 overflow-hidden bg-background"
        onContextMenu={openMenu}
        inert={overlayOpen || undefined}
      >
        <AppSidebar />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-hidden">
            <Suspense fallback={<RouteFallback />}>
              {stageElement}
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
      </div>
      {menu ? (
        <div
          ref={menuRef}
          role="menu"
          aria-label={menu.kind === 'field' ? t('textMenu.label') : t('textMenu.contentLabel')}
          className="fixed z-[100] w-56 rounded-xl border border-border bg-popover p-1.5 text-popover-foreground shadow-lg"
          style={{ left: menu.x, top: menu.y }}
        >
          {menuItems.map((item) => (
            <button
              key={item.key}
              type="button"
              role="menuitem"
              disabled={item.disabled}
              className="flex h-9 w-full items-center justify-between rounded-lg px-3 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent disabled:pointer-events-none disabled:opacity-40"
              onClick={item.run}
            >
              <span>{item.label}</span>
              {item.shortcut ? (
                <span className="ml-8 text-xs text-muted-foreground">{item.shortcut}</span>
              ) : null}
            </button>
          ))}
        </div>
      ) : null}
      {overlayOpen ? (
        <OverlayProvider value={{ background: stage }}>
          <SettingsOverlay
            label={overlayLabel ?? ''}
            onClose={closeOverlay}
            onContextMenu={openMenu}
          >
            {overlayElement}
          </SettingsOverlay>
        </OverlayProvider>
      ) : null}
      </TerminalRuntimeProvider>
    </UnsavedChangesProvider>
    </CommandPaletteProvider>
  )
}
