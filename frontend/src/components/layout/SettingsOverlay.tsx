import { useEffect, useRef, type MouseEvent as ReactMouseEvent, type ReactNode } from 'react'

import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'
import { useUnsavedChangesAction } from '@/hooks/useUnsavedChangesGuard'
import { cn } from '@/lib/utils'

const GROUP_DRAWER_WIDTH_STORAGE_KEY = 'ag-swarmer:layout:group-settings-drawer-width'

const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

export interface SettingsOverlayProps {
  /** Accessible name for the panel — the area it is showing. */
  label: string
  /** Back / Escape. */
  onClose: () => void
  /**
   * The shell's right-click menu. The overlay lives outside `AppLayout`'s
   * contextmenu-marker because that wrapper is inert and the overlay is not;
   * forwarding this keeps text editing copy/paste working inside settings.
   */
  onContextMenu?: (event: ReactMouseEvent<HTMLDivElement>) => void
  /** Group management uses a compact sheet; resource settings use the inset panel. */
  variant?: 'panel' | 'drawer'
  /** Accessible label for the group drawer's resize separator. */
  resizeLabel?: string
  children: ReactNode
}

/**
 * The panel a settings-class area renders in when it floats over a
 * conversation.
 *
 * Layering: `z-40`. Above the Assistant dock (`z-[30]`), which the panel covers
 * and `AppLayout` marks inert while this is open. Below the `z-50` Radix
 * overlays, so a confirm dialog opened from inside settings lands on top of the
 * panel rather than behind it. Below the `z-[100]` text context menu in
 * `AppLayout` too, so right-click copy still works in here. The terminal dock
 * is in normal flow inside the stage, so it is under all of these.
 *
 * Resource settings stay near full height and width because they contain wide
 * forms and master/detail views. Group management opts into the compact drawer
 * variant and adapts its own content to the narrower measure.
 *
 * Clicking the scrim does not close. The scrim is a narrow frame around an
 * almost-full-screen panel, so a click landing there is nearly always a miss —
 * and behind these forms are half-typed API keys and prompts.
 */
export function SettingsOverlay({
  label,
  onClose,
  onContextMenu,
  variant = 'panel',
  resizeLabel,
  children,
}: SettingsOverlayProps) {
  const panelRef = useRef<HTMLDivElement>(null)
  const closeRef = useRef(onClose)
  const requestAction = useUnsavedChangesAction()
  const requestActionRef = useRef(requestAction)
  closeRef.current = onClose
  requestActionRef.current = requestAction
  const drawerWidth = usePersistentPaneWidth({
    storageKey: GROUP_DRAWER_WIDTH_STORAGE_KEY,
    defaultWidth: 512,
    minWidth: 400,
    maxWidth: 960,
  })

  useEffect(() => {
    const panel = panelRef.current
    if (!panel) return
    const previous = document.activeElement
    panel.focus({ preventScroll: true })

    // Native listener, not React's: Radix portals a nested dialog to
    // `document.body`, and a React handler here would still see its key events
    // through the portal — closing the whole panel on the Escape meant for the
    // dialog. A DOM listener on this element only hears what is really inside.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        requestActionRef.current(closeRef.current)
        return
      }
      if (event.key !== 'Tab') return
      // Only cycle while focus is genuinely in the panel: an open select or
      // popover runs its own trap, and stealing focus back would shut it.
      if (!panel.contains(document.activeElement)) return
      const items = [...panel.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
        (node) => node.getAttribute('aria-hidden') !== 'true',
      )
      if (items.length === 0) {
        event.preventDefault()
        panel.focus({ preventScroll: true })
        return
      }
      const first = items[0]!
      const last = items[items.length - 1]!
      if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      } else if (event.shiftKey && (document.activeElement === first || document.activeElement === panel)) {
        event.preventDefault()
        last.focus()
      }
    }

    panel.addEventListener('keydown', onKeyDown)
    return () => {
      panel.removeEventListener('keydown', onKeyDown)
      // Hand focus back to whatever opened the panel, so closing settings from
      // the keyboard returns the caret to the sidebar row it was launched from.
      if (previous instanceof HTMLElement && previous.isConnected) {
        previous.focus({ preventScroll: true })
      }
    }
  }, [])

  return (
    <div
      className={cn(
        'fixed inset-0 z-40 flex',
        variant === 'drawer' ? 'justify-end' : 'p-2 sm:p-4',
      )}
      onContextMenu={onContextMenu}
    >
      <div
        className={cn(
          'animate-overlay-scrim absolute inset-0',
          variant === 'drawer' ? 'group-drawer-scrim' : 'bg-scrim',
        )}
        aria-hidden
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        data-variant={variant}
        tabIndex={-1}
        style={variant === 'drawer' ? { width: drawerWidth.width } : undefined}
        className={cn(
          'relative flex min-h-0 flex-col overflow-hidden border-border bg-background outline-none',
          variant === 'drawer'
            ? 'animate-overlay-drawer h-full w-full max-w-full border-l shadow-2xl'
            : 'animate-overlay-panel w-full flex-1 rounded-lg border shadow-lg',
        )}
      >
        {variant === 'drawer' ? (
          <VerticalResizeHandle
            label={resizeLabel ?? label}
            value={drawerWidth.width}
            min={drawerWidth.minWidth}
            max={drawerWidth.maxWidth}
            increaseOnArrowRight={false}
            onResizeStart={(event) => drawerWidth.startResize(event, -1)}
            onStep={drawerWidth.resizeBy}
            className="absolute inset-y-0 left-0 z-30"
          />
        ) : null}
        {children}
      </div>
    </div>
  )
}
