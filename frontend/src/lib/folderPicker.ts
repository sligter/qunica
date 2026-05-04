/**
 * Browser folder picker shim.
 *
 * Browsers cannot expose absolute filesystem paths for privacy reasons. We
 * therefore use the picker only to capture the picked folder's *name*, and
 * combine it with a remembered "absolute prefix" the user typed previously
 * (kept in localStorage) to compose a likely full path. The user can always
 * edit the result.
 *
 * Modern browsers expose `window.showDirectoryPicker()` (Chromium-based);
 * non-supported browsers must use the legacy `<input type="file"
 * webkitdirectory>` element themselves and forward the picked file's
 * `webkitRelativePath` here.
 *
 * No file content is uploaded or read. Only the picked folder name is
 * surfaced.
 */

interface FileSystemDirectoryHandleLike {
  readonly name: string
}

interface ShowDirectoryPickerWindow extends Window {
  showDirectoryPicker?: () => Promise<FileSystemDirectoryHandleLike>
}

export type FolderPickResult =
  | { kind: 'native'; name: string }
  | { kind: 'cancelled' }
  | { kind: 'fallback' }

const SEPARATOR_RE = /[\\/]/
const ABSOLUTE_PREFIX_RE = /^(?:[A-Za-z]:[\\/]|\\\\|\/)/
const TRAILING_SEPARATOR_RE = /[\\/]+$/

export async function pickFolder(): Promise<FolderPickResult> {
  const showDirectoryPicker = (window as ShowDirectoryPickerWindow)
    .showDirectoryPicker
  if (typeof showDirectoryPicker !== 'function') {
    return { kind: 'fallback' }
  }
  try {
    const handle = await showDirectoryPicker()
    return { kind: 'native', name: handle.name }
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') {
      return { kind: 'cancelled' }
    }
    return { kind: 'fallback' }
  }
}

function detectSeparator(path: string): string {
  if (path.includes('\\')) return '\\'
  if (path.includes('/')) return '/'
  return '/'
}

export function looksAbsolute(path: string): boolean {
  return ABSOLUTE_PREFIX_RE.test(path.trim())
}

export function dirname(path: string): string {
  const trimmed = path.replace(TRAILING_SEPARATOR_RE, '')
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  if (idx <= 0) return ''
  return trimmed.slice(0, idx)
}

export function basename(path: string): string {
  const trimmed = path.replace(TRAILING_SEPARATOR_RE, '')
  const parts = trimmed.split(SEPARATOR_RE).filter(Boolean)
  return parts[parts.length - 1] ?? trimmed
}

/**
 * Compose a path candidate for the picker input.
 *
 * Rules:
 * - If the current input is an absolute path with a separator, replace its
 *   trailing folder segment with the picked name.
 * - Else, if a remembered prefix exists, return `<prefix><sep><name>`.
 * - Else, fall back to the bare picked name (best effort).
 */
export function composePickedPath(
  current: string,
  pickedName: string,
  rememberedPrefix: string | null,
): string {
  const name = pickedName.trim()
  if (!name) return current

  const trimmedCurrent = current.trim()
  if (looksAbsolute(trimmedCurrent) && SEPARATOR_RE.test(trimmedCurrent)) {
    const sep = detectSeparator(trimmedCurrent)
    const parent = dirname(trimmedCurrent)
    return parent ? `${parent}${sep}${name}` : `${trimmedCurrent}${sep}${name}`
  }

  const prefix = rememberedPrefix?.trim()
  if (prefix && looksAbsolute(prefix)) {
    const sep = detectSeparator(prefix)
    return `${prefix.replace(TRAILING_SEPARATOR_RE, '')}${sep}${name}`
  }

  return name
}

const PREFIX_STORAGE_PREFIX = 'ag-swarmer/picker-prefix:'

function storageKey(scope: string): string {
  return `${PREFIX_STORAGE_PREFIX}${scope}`
}

export function readRememberedPrefix(scope: string): string | null {
  try {
    return window.localStorage.getItem(storageKey(scope))
  } catch {
    return null
  }
}

/**
 * Save the directory portion of `value` if it looks like an absolute path
 * with a separator; ignored otherwise. Empty strings clear the entry.
 */
export function saveRememberedPrefix(scope: string, value: string): void {
  try {
    const trimmed = value.trim()
    if (!trimmed) {
      window.localStorage.removeItem(storageKey(scope))
      return
    }
    if (!looksAbsolute(trimmed) || !SEPARATOR_RE.test(trimmed)) return
    const parent = dirname(trimmed)
    if (parent) {
      window.localStorage.setItem(storageKey(scope), parent)
    }
  } catch {
    // localStorage unavailable; ignore.
  }
}
