import { useEffect, useRef, type MouseEvent as ReactMouseEvent, type ReactNode } from 'react'

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
 * Near full height and full width rather than a right-hand sheet: the content
 * is wide forms and two-column master/detail views, which a sheet would force
 * into a single squeezed column.
 *
 * Clicking the scrim does not close. The scrim is a narrow frame around an
 * almost-full-screen panel, so a click landing there is nearly always a miss —
 * and behind these forms are half-typed API keys and prompts.
 */
export function SettingsOverlay({ label, onClose, onContextMenu, children }: SettingsOverlayProps) {
  const panelRef = useRef<HTMLDivElement>(null)
  const closeRef = useRef(onClose)
  closeRef.current = onClose

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
        closeRef.current()
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
    <div className="fixed inset-0 z-40 flex p-2 sm:p-4" onContextMenu={onContextMenu}>
      <div className="animate-overlay-scrim absolute inset-0 bg-scrim" aria-hidden />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
        className="animate-overlay-panel relative flex min-h-0 w-full flex-1 flex-col overflow-hidden rounded-lg border border-border bg-background shadow-lg outline-none"
      >
        {children}
      </div>
    </div>
  )
}
