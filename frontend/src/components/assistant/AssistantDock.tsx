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
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from '@/components/ui/sheet'
import { useCompactLayout } from '@/hooks/useMediaQuery'
import { useMobilePanel } from '@/components/layout/mobilePanels'
import { useAssistant } from '@/hooks/useAssistant'
import { useProvider } from '@/hooks/useProviders'
import { cn } from '@/lib/utils'
import { toggleAssistantWindow } from '@/lib/desktop'
import { isDesktopRuntime } from '@/lib/runtime'
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
  // Desktop shells promote the dock to an always-on-top native window: a DOM
  // overlay cannot leave the webview that hosts it, and staying above every
  // application window is exactly what the utility is for there. The browser
  // build keeps the in-page panel below.
  const desktop = isDesktopRuntime()
  const compactLayout = useCompactLayout()
  const [mobileOpen, setMobileOpen] = useMobilePanel('assistant')
  const { placement, setPlacement, snapToNearestCorner, toggleCollapsed } =
    useAssistantDockPlacement()
  const launcherRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  // Set while collapsing so focus returns to the launcher only when the user
  // collapsed it, not on every mount.
  const restoreFocusRef = useRef(false)

  const assistant = useAssistant()
  const [showSettings, setShowSettings] = useState(false)
  // The Assistant's model is set in its own settings panel, not per message.
  // Only how hard it thinks stays a per-question choice.
  const provider = useProvider(assistant.data?.provider_id ?? undefined)
  const activeModel = assistant.data?.model ?? provider.data?.default_model
  const supportsReasoningEffort = Boolean(
    provider.data?.models?.find((model) => model.id === activeModel)?.supports_reasoning_effort,
  )
  const expanded = !compactLayout && !placement.collapsed
  const ready = assistant.data?.provider_configured === true
  const status = showSettings
    ? t('status.settings')
    : assistant.isLoading
      ? t('status.loading')
      : assistant.error
        ? t('status.error')
        : ready
          ? activeModel ?? t('status.ready')
          : t('status.setup')

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

  const content = showSettings ? (
    <AssistantSettings onClose={() => setShowSettings(false)} />
  ) : assistant.data?.provider_configured && assistant.data.chat_id ? (
    <ConversationChatView conversationId={assistant.data.chat_id} workspaceId={null}
      supportsReasoningEffort={supportsReasoningEffort} scope="direct-chats" agents={[]}
      agentIsSystem compact title={t('chat.title')}
      capabilities={{ showAnnouncement: false, showManage: false, showTurnTrace: false,
        showWorkspace: false, showTerminal: false, allowMentions: false }} />
  ) : <AssistantSetupChecklist loading={assistant.isLoading} error={Boolean(assistant.error)} />

  if (compactLayout && !desktop) {
    return (
      <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
        <SheetTrigger asChild>
          <Button variant="ghost" size="icon" className="fixed right-2 top-[max(0.25rem,env(safe-area-inset-top))] z-30" aria-label={t('title')}>
            <Sparkles className="h-5 w-5 text-primary" aria-hidden />
          </Button>
        </SheetTrigger>
        <SheetContent className="mobile-sheet top-auto w-full rounded-t-2xl border-t" style={{ height: '92dvh' }} closeLabel={t('collapse')} aria-describedby={undefined}>
          <div className="flex h-14 shrink-0 items-center gap-3 border-b border-border px-4 pr-16">
            <SheetTitle className="flex-1">{t('title')}</SheetTitle>
            <Button variant="ghost" size="icon" aria-label={t('settings.title')} aria-pressed={showSettings} onClick={() => setShowSettings(open => !open)}>
              <Settings2 className="h-4 w-4" />
            </Button>
          </div>
          <div className="min-h-0 flex-1 overflow-hidden">{content}</div>
        </SheetContent>
      </Sheet>
    )
  }

  if (desktop) {
    return createPortal(
      <button
        type="button"
        aria-label={t('title')}
        title={t('title')}
        onClick={() => void toggleAssistantWindow().catch(() => undefined)}
        className="fixed bottom-4 right-4 z-[30] flex h-12 w-12 items-center justify-center rounded-2xl border border-primary/30 bg-primary text-primary-foreground shadow-lg outline-none transition-[transform,background-color,box-shadow] hover:-translate-y-0.5 hover:bg-primary/90 hover:shadow-xl focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
      >
        <Sparkles className="h-5 w-5" aria-hidden />
      </button>,
      document.body,
    )
  }

  if (placement.collapsed) {
    return createPortal(
      <button
        ref={launcherRef}
        type="button"
        aria-label={t('title')}
        title={t('title')}
        onClick={toggleCollapsed}
        className="fixed bottom-4 right-4 z-[30] flex h-12 w-12 items-center justify-center rounded-2xl border border-primary/30 bg-primary text-primary-foreground shadow-lg outline-none transition-[transform,background-color,box-shadow] hover:-translate-y-0.5 hover:bg-primary/90 hover:shadow-xl focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
      >
        <Sparkles className="h-5 w-5" aria-hidden />
        <span
          aria-hidden
          className={cn(
            'absolute right-1.5 top-1.5 h-2 w-2 rounded-full border-2 border-primary',
            assistant.error
              ? 'bg-destructive'
              : ready
                ? 'bg-primary-foreground'
                : 'bg-warning-foreground',
          )}
        />
      </button>,
      document.body,
    )
  }

  return createPortal(
    <div
      ref={panelRef}
      role="dialog"
      aria-label={t('title')}
      className="fixed z-[30] flex flex-col overflow-hidden rounded-2xl border border-border/80 bg-card shadow-2xl"
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
        className="flex shrink-0 cursor-grab items-center justify-between gap-2 border-b border-border/70 bg-card px-3 py-2.5 active:cursor-grabbing"
      >
        <div className="flex min-w-0 items-center gap-2">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
            <Bot className="h-4 w-4" aria-hidden />
          </span>
          <span className="min-w-0">
            <span className="block truncate font-serif text-sm font-semibold tracking-tight">
              {t('title')}
            </span>
            <span className="flex min-w-0 items-center gap-1.5 text-2xs text-muted-foreground">
              <span
                aria-hidden
                className={cn(
                  'h-1.5 w-1.5 shrink-0 rounded-full',
                  assistant.isLoading
                    ? 'bg-muted-foreground motion-safe:animate-pulse'
                    : assistant.error
                      ? 'bg-destructive'
                      : ready
                        ? 'bg-success'
                        : 'bg-warning-foreground',
                )}
              />
              <span className="truncate" title={status} aria-live="polite">
                {status}
              </span>
            </span>
          </span>
        </div>
        <div
          className="flex shrink-0 items-center gap-0.5"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <Button
            type="button"
            variant={showSettings ? 'secondary' : 'ghost'}
            size="icon"
            className="h-8 w-8 rounded-full"
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
            className="h-8 w-8 rounded-full"
            aria-label={t('collapse')}
            onClick={collapse}
          >
            <Minus className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden bg-background">
        {content}
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
