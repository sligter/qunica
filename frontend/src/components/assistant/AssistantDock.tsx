/**
 * The Assistant's floating panel.
 *
 * An in-app DOM overlay rather than a second Tauri window: a second webview
 * would need its own IPC bridge, its own token, and its own always-on-top
 * handling, and would not exist at all in the browser build.
 *
 * Layering is deliberate. The dock sits at `z-[90]`: above the terminal dock
 * and the workspace panel, below the `z-50` Radix overlays and the `z-[100]`
 * text context menu in `AppLayout`, so dialogs and right-click menus still
 * cover it rather than fighting it.
 */

import { useCallback, useEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react'
import { createPortal } from 'react-dom'
import { Bot, Minus, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantSetupChecklist } from '@/components/assistant/AssistantSetupChecklist'
import { useAssistantDockPlacement } from '@/components/assistant/useAssistantDockPlacement'
import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { Button } from '@/components/ui/button'
import { useAssistant } from '@/hooks/useAssistant'
import { useProvider } from '@/hooks/useProviders'
import { useAuthStore } from '@/stores/authStore'

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
    (event: ReactPointerEvent<HTMLElement>) => {
      if (event.button !== 0) return
      event.preventDefault()
      event.stopPropagation()
      const startX = event.clientX
      const startY = event.clientY
      const originWidth = placement.width
      const originHeight = placement.height
      const originX = placement.x

      const previousCursor = document.body.style.cursor
      const previousSelect = document.body.style.userSelect
      document.body.style.cursor = 'nwse-resize'
      document.body.style.userSelect = 'none'

      const onMove = (move: PointerEvent) => {
        // The handle is on the leading (left) edge, so growing means the panel
        // extends leftwards and its origin moves with it.
        const deltaX = startX - move.clientX
        setPlacement({
          width: originWidth + deltaX,
          height: originHeight + (move.clientY - startY),
          x: originX - deltaX,
        })
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
    [placement.height, placement.width, placement.x, setPlacement],
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
        className="fixed bottom-4 right-4 z-[90] flex h-11 w-11 items-center justify-center rounded-full border border-border/70 bg-background text-foreground shadow-lg outline-none transition hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
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
      className="fixed z-[90] flex flex-col overflow-hidden rounded-xl border border-border/70 bg-background shadow-2xl"
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

      <div className="min-h-0 flex-1 overflow-hidden">
        {assistant.data?.provider_configured && assistant.data.chat_id ? (
          <ConversationChatView
            conversationId={assistant.data.chat_id}
            workspaceId={null}
            models={models}
            defaultModel={provider.data?.default_model}
            scope="direct-chats"
            schedulerEnabled={false}
            agents={[]}
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

      <div
        onPointerDown={startResize}
        aria-hidden
        className="absolute bottom-0 left-0 h-4 w-4 cursor-nesw-resize"
      />
    </div>,
    document.body,
  )
}
