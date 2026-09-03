/**
 * Folder picker shim.
 *
 * Only the desktop shell can open a native dialog, because only there is the
 * filesystem the user is browsing the same one the backend will open. In a
 * browser the OS picker shows the *client's* folders — on a Docker or VPS
 * deployment those directories do not exist on the server at all, and the
 * browser will not reveal their absolute paths anyway. So the browser build
 * asks the server what it has instead; see `ServerFolderPicker`.
 */

import { isDesktopRuntime } from '@/lib/runtime'

export type FolderPickResult =
  | { kind: 'native'; name: string; path?: string }
  | { kind: 'cancelled' }
  | { kind: 'error'; message: string }
  /** Not a desktop runtime: the caller should open the server browser. */
  | { kind: 'serverBrowse' }

const SEPARATOR_RE = /[\\/]/
const ABSOLUTE_PREFIX_RE = /^(?:[A-Za-z]:[\\/]|\\\\|\/)/
const TRAILING_SEPARATOR_RE = /[\\/]+$/
const WINDOWS_DEVICE_PREFIX = '\\\\?\\'
const WINDOWS_UNC_DEVICE_PREFIX = `${WINDOWS_DEVICE_PREFIX}UNC\\`

export function normalizeWindowsPath(path: string): string {
  if (path.startsWith(WINDOWS_UNC_DEVICE_PREFIX)) {
    return `\\\\${path.slice(WINDOWS_UNC_DEVICE_PREFIX.length)}`
  }
  return path.startsWith(WINDOWS_DEVICE_PREFIX)
    ? path.slice(WINDOWS_DEVICE_PREFIX.length)
    : path
}

export async function pickFolder(): Promise<FolderPickResult> {
  if (!isDesktopRuntime()) {
    return { kind: 'serverBrowse' }
  }
  const tauriPath = await pickTauriFolder()
  if (tauriPath === null) {
    return { kind: 'cancelled' }
  }
  if (tauriPath) {
    return { kind: 'native', name: basename(tauriPath), path: tauriPath }
  }
  return {
    kind: 'error',
    message: 'Desktop folder picker did not return a local filesystem path.',
  }
}

async function pickTauriFolder(): Promise<string | null | undefined> {
  const commandPath = await pickTauriFolderCommand()
  if (commandPath !== undefined) return commandPath
  try {
    const dialog = await import('@tauri-apps/plugin-dialog')
    const selected = await dialog.open({ directory: true, multiple: false })
    if (selected === null) return null
    return coerceTauriPath(selected)
  } catch {
    return undefined
  }
}

async function pickTauriFolderCommand(): Promise<string | null | undefined> {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const selected = await invoke<unknown>('pick_workspace_folder')
    return coerceTauriPath(selected)
  } catch {
    return undefined
  }
}

function coerceTauriPath(value: unknown): string | null | undefined {
  if (value === null) return null
  if (typeof value === 'string') return normalizeWindowsPath(value)
  if (Array.isArray(value)) return coerceTauriPath(value[0])
  if (typeof value !== 'object' || value === null) return undefined
  const record = value as Record<string, unknown>
  for (const key of ['path', 'Path', 'filePath', 'FilePath']) {
    const nested = record[key]
    if (typeof nested === 'string') return normalizeWindowsPath(nested)
  }
  return undefined
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

const PREFIX_STORAGE_PREFIX = 'qunica/picker-prefix:'

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
