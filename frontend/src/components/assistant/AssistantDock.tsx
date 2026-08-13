/**
 * The Assistant's floating panel.
 *
 * An in-app DOM overlay rather than a second Tauri window: a second webview
 * would need its own IPC bridge, its own token, and its own always-on-top
 * handling, and would not exist at all in the browser build.
 *
 * Layering is deliberate. The dock sits at `z-[30]`: above the ordinary page
 * content it floats over, but below the `z-50` Radix overlays and the `z-[100]`
 * text context menu in `AppLayout`.
 *
 * Below, not above. Radix portals its dialogs to `document.body`, so they are
 * siblings of this panel rather than descendants — a dock stacked above them
 * covers its own confirm dialogs, which then render visibly but cannot be
 * clicked.
 */

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { createPortal } from 'react-dom'
import { Bot, Minus, Settings2, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantSettings } from '@/components/assistant/AssistantSettings'
import { AssistantSetupChecklist } from '@/components/assistant/AssistantSetupChecklist'
import {
  MIN_DOCK_HEIGHT,
  MIN_DOCK_WIDTH,
  useAssistantDockPlacement,
} from '@/components/assistant/useAssistantDockPlacement'
import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { Button } from '@/components/ui/button'
import { useAssistant } from '@/hooks/useAssistant'
import { useProvider } from '@/hooks/useProviders'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/authStore'

/** Every edge and corner the panel can be resized from. */
const RESIZE_DIRECTIONS = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as const
type ResizeDirection = (typeof RESIZE_DIRECTIONS)[number]

const RESIZE_CURSORS: Record<ResizeDirection, string> = {
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
  nw: 'nwse-resize',
  se: 'nwse-resize',
}

/**
 * Hit areas. Edges are 6px thick and inset past the corners so a corner drag
 * is never captured by the edge lying under it; corners are 14px squares.
 */
const RESIZE_HANDLE_CLASSES: Record<ResizeDirection, string> = {
  n: 'left-3 right-3 top-0 h-1.5',
  s: 'left-3 right-3 bottom-0 h-1.5',
  e: 'top-3 bottom-3 right-0 w-1.5',
  w: 'top-3 bottom-3 left-0 w-1.5',
  ne: 'top-0 right-0 h-3.5 w-3.5',
  nw: 'top-0 left-0 h-3.5 w-3.5',
  se: 'bottom-0 right-0 h-3.5 w-3.5',
  sw: 'bottom-0 left-0 h-3.5 w-3.5',
}

function clampSize(value: number, minimum: number): number {
  return Math.max(minimum, value)
}

export function AssistantDock() {
  const { t } = useTranslation('assistant')
  const token = useAuthStore((state) => state.token)
  const { placement, setPlacement, snapToNearestCorner, toggleCollapsed } =
    useAssistantDockPlacement()
  const launcherRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  // Set while collapsing so focus returns to the launcher only when the user
  // collapsed it, not on every mount.
  const restoreFocusRef = useRef(false)

  const assistant = useAssistant()
  const [showSettings, setShowSettings] = useState(false)
  // The dock is a single-agent conversation, so a per-message model is
  // unambiguous here too.
  const provider = useProvider(assistant.data?.provider_id ?? undefined)
  const models = provider.data?.models?.map((model) => ({ id: model.id })) ?? []
  const expanded = !placement.collapsed

  const collapse = useCallback(() => {
    restoreFocusRef.current = true
    setPlacement({ collapsed: true })
  }, [setPlacement])

  useEffect(() => {
    if (placement.collapsed && restoreFocusRef.current) {
      restoreFocusRef.current = false
      launcherRef.current?.focus()
    }
  }, [placement.collapsed])

  useEffect(() => {
    if (!expanded) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      // Let a dialog or popover inside the dock handle its own Escape first.
      if (document.querySelector('[role="alertdialog"], [data-state="open"][role="dialog"]')) {
        return
      }
      collapse()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [collapse, expanded])

  const startDrag = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      // Pointer events, not HTML5 drag: marking anything here draggable would
      // interfere with the workspace file drag the composer relies on.
      if (event.button !== 0) return
      event.preventDefault()
      const startX = event.clientX
      const startY = event.clientY
      const originX = placement.x
      const originY = placement.y

      const onMove = (move: PointerEvent) => {
        setPlacement({
          x: originX + (move.clientX - startX),
          y: originY + (move.clientY - startY),
        })
      }
      const onUp = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', onUp)
        snapToNearestCorner()
      }
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', onUp)
    },
    [placement.x, placement.y, setPlacement, snapToNearestCorner],
  )

  const startResize = useCallback(
    (event: ReactPointerEvent<HTMLElement>, direction: ResizeDirection) => {
      if (event.button !== 0) return
      event.preventDefault()
      event.stopPropagation()
      const startX = event.clientX
      const startY = event.clientY
      const origin = { ...placement }

      const previousCursor = document.body.style.cursor
      const previousSelect = document.body.style.userSelect
      document.body.style.cursor = RESIZE_CURSORS[direction]
      document.body.style.userSelect = 'none'

      const onMove = (move: PointerEvent) => {
        const deltaX = move.clientX - startX
        const deltaY = move.clientY - startY
        let { x, y, width, height } = origin

        // Dragging a leading edge moves the origin as well as the size. Clamp
        // the size first and derive the origin from it, so hitting the minimum
        // pins the trailing edge instead of sliding the whole panel along.
        if (direction.includes('e')) {
          width = origin.width + deltaX
        } else if (direction.includes('w')) {
          width = clampSize(origin.width - deltaX, MIN_DOCK_WIDTH)
          x = origin.x + (origin.width - width)
        }
        if (direction.includes('s')) {
          height = origin.height + deltaY
        } else if (direction.includes('n')) {
          height = clampSize(origin.height - deltaY, MIN_DOCK_HEIGHT)
          y = origin.y + (origin.height - height)
        }

        setPlacement({ x, y, width, height })
      }
      const onUp = () => {
        document.body.style.cursor = previousCursor
        document.body.style.userSelect = previousSelect
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', onUp)
      }
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', onUp)
    },
    [placement, setPlacement],
  )

  // Signed out means the login and register routes, where a floating helper
  // would be both useless and unauthenticated.
  if (!token) return null

  if (placement.collapsed) {
    return createPortal(
      <button
        ref={launcherRef}
        type="button"
        aria-label={t('title')}
        title={t('title')}
        onClick={toggleCollapsed}
        className="fixed bottom-4 right-4 z-[30] flex h-11 w-11 items-center justify-center rounded-full border border-border/70 bg-background text-foreground shadow-lg outline-none transition hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Sparkles className="h-5 w-5" aria-hidden />
      </button>,
      document.body,
    )
  }

  return createPortal(
    <div
      ref={panelRef}
      role="dialog"
      aria-label={t('title')}
      className="fixed z-[30] flex flex-col overflow-hidden rounded-xl border border-border/70 bg-background shadow-2xl"
      style={{
        left: placement.x,
        top: placement.y,
        width: placement.width,
        height: placement.height,
      }}
    >
      <div
        data-testid="assistant-dock-drag-handle"
        onPointerDown={startDrag}
        className="flex shrink-0 cursor-grab items-center justify-between gap-2 border-b border-border/70 bg-muted/40 px-3 py-2 active:cursor-grabbing"
      >
        <div className="flex min-w-0 items-center gap-2">
          <Bot className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
          <span className="truncate text-sm font-medium">{t('title')}</span>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            aria-label={t('settings.title')}
            aria-pressed={showSettings}
            onClick={() => setShowSettings((open) => !open)}
          >
            <Settings2 className="h-4 w-4" aria-hidden />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            aria-label={t('collapse')}
            onClick={collapse}
          >
            <Minus className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        {showSettings ? (
          <AssistantSettings onClose={() => setShowSettings(false)} />
        ) : assistant.data?.provider_configured && assistant.data.chat_id ? (
          <ConversationChatView
            conversationId={assistant.data.chat_id}
            workspaceId={null}
            models={models}
            defaultModel={provider.data?.default_model}
            scope="direct-chats"
            agents={[]}
            agentIsSystem
            title={<span className="text-sm font-medium">{t('title')}</span>}
            capabilities={{
              showAnnouncement: false,
              showManage: false,
              showTurnTrace: false,
              // The Assistant has no workspace, so a file panel would only ever
              // report that none is configured.
              showWorkspace: false,
              allowMentions: false,
            }}
          />
        ) : (
          <AssistantSetupChecklist
            loading={assistant.isLoading}
            error={Boolean(assistant.error)}
          />
        )}
      </div>

      {/* Edges and corners, each with a hit area wider than the visible border
          so the pointer does not have to land on a hairline. */}
      {RESIZE_DIRECTIONS.map((direction) => (
        <div
          key={direction}
          data-testid={`assistant-dock-resize-${direction}`}
          onPointerDown={(event) => startResize(event, direction)}
          aria-hidden
          className={cn('absolute touch-none', RESIZE_HANDLE_CLASSES[direction])}
          style={{ cursor: RESIZE_CURSORS[direction] }}
        />
      ))}
    </div>,
    document.body,
  )
}
