/**
 * Desktop-only (Tauri) bridges for OS file operations.
 *
 * All helpers dynamically import the Tauri API so the module stays inert in the
 * browser build. Guard calls with `isDesktopRuntime()` from `@/lib/runtime`.
 */

import { isDesktopRuntime } from '@/lib/runtime'
import type { SystemLogSnapshot } from '@/lib/systemLogs'

export { isDesktopRuntime }

export const LIBRARY_WINDOW_LABEL = 'library'
export const SETTINGS_WINDOW_LABEL = 'settings'
export const ASSISTANT_WINDOW_LABEL = 'assistant'

/**
 * Which native window this webview belongs to.
 *
 * Auxiliary windows load the same SPA as the conversation, so the route tree
 * needs a second signal to drop the main sidebar and assistant launcher.
 * Browser builds always report `main`.
 */
export function desktopWindowLabel(): string {
  if (typeof window === 'undefined' || !isDesktopRuntime()) return 'main'
  const metadata = (window as Window & {
    __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } }
  }).__TAURI_INTERNALS__
  return metadata?.metadata?.currentWindow?.label ?? 'main'
}

export function isAuxiliaryDesktopWindow(): boolean {
  const label = desktopWindowLabel()
  return label === LIBRARY_WINDOW_LABEL || label === SETTINGS_WINDOW_LABEL
}

export function isLibraryDesktopWindow(): boolean {
  return desktopWindowLabel() === LIBRARY_WINDOW_LABEL
}

export function isSettingsDesktopWindow(): boolean {
  return desktopWindowLabel() === SETTINGS_WINDOW_LABEL
}

export function isAssistantDesktopWindow(): boolean {
  return desktopWindowLabel() === ASSISTANT_WINDOW_LABEL
}

export async function closeCurrentDesktopWindow(): Promise<void> {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  await getCurrentWindow().close()
}

/** Hide the current native window without destroying its mounted SPA state. */
export async function hideCurrentDesktopWindow(): Promise<void> {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  await getCurrentWindow().hide()
}

/** Begin an OS-native move gesture for the current undecorated window. */
export async function startDraggingCurrentDesktopWindow(): Promise<void> {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  await getCurrentWindow().startDragging()
}

/** Reveal (and select) an absolute path in the OS file manager. */
export async function revealInFileManager(absPath: string): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('reveal_in_file_manager', { path: absPath })
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunkSize = 0x8000
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize))
  }
  return btoa(binary)
}

/**
 * Show a native "Save As" dialog and write the bytes to the chosen path.
 * Returns the saved absolute path, or null if the user cancelled.
 */
export async function saveFileViaDialog(
  name: string,
  data: Uint8Array,
): Promise<string | null> {
  const { invoke } = await import('@tauri-apps/api/core')
  const result = await invoke<string | null>('save_file', {
    name,
    contentsB64: bytesToBase64(data),
  })
  return result ?? null
}

export async function getSystemLogs(): Promise<SystemLogSnapshot> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<SystemLogSnapshot>('system_logs_snapshot')
}

export async function setSystemLogFilter(filter: string): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('set_system_log_filter', { filter })
}

export async function clearSystemLogs(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('clear_system_logs')
}

export async function openSystemLogsFolder(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('open_system_logs_folder')
}

/** Open a resource-library route as an independent top-level window. */
export async function openLibraryWindow(route = '/agents'): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('open_library_window', { route })
}

/** Open a settings route as an independent top-level window. */
export async function openSettingsWindow(route = '/settings/system'): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('open_settings_window', { route })
}

/** Show/focus or hide the always-on-top Assistant utility window. */
export async function toggleAssistantWindow(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('toggle_assistant_window')
}
