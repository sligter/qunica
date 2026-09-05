import { useEffect } from 'react'

/**
 * Same set AppLayout's editing menu recognises. The two policies have to agree:
 * one decides where the webview's menu survives, the other decides where the
 * app's own menu takes over.
 */
const TEXT_INPUT_TYPES = new Set(['email', 'password', 'search', 'tel', 'text', 'url'])

/**
 * A text field, or anything else the user types into. Editable surfaces keep
 * their native menu because that is where the IME and the OS hang paste,
 * candidate, and correction commands — none of which a DOM menu can reproduce.
 */
function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false
  const field = target.closest('input, textarea, [contenteditable=""], [contenteditable="true"]')
  if (field === null) return false
  if (field instanceof HTMLTextAreaElement) return true
  if (field instanceof HTMLInputElement) return TEXT_INPUT_TYPES.has(field.type)
  return true
}

/**
 * Kills the webview's own page menu app-wide — reload, back, view source, and
 * the rest belong to a browser, not to a desktop app that ships its own chrome.
 *
 * Listening on the document rather than a wrapper element is what makes this
 * complete: it also covers the signed-out routes, the 404, and everything that
 * portals to <body> (dialogs, sheets, the assistant dock), none of which sit
 * inside AppLayout's tree. Surfaces that publish their own menu call
 * preventDefault first and are unaffected — this only closes the gaps.
 */
export function useSuppressNativeContextMenu(): void {
  useEffect(() => {
    const suppress = (event: MouseEvent) => {
      if (window.matchMedia && !window.matchMedia('(pointer: fine)').matches) return
      if (isEditable(event.target)) return
      event.preventDefault()
    }
    document.addEventListener('contextmenu', suppress)
    return () => document.removeEventListener('contextmenu', suppress)
  }, [])
}
