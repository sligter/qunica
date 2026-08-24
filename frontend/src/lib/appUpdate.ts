/**
 * About-this-build and updater bridge for the desktop shell.
 *
 * Every call is inert outside Tauri: a browser tab has no build to name and
 * nothing to install into, and it picks up new code on its next reload anyway.
 * Callers get an explicit `unsupported` result rather than an error, so the
 * settings page can say so instead of rendering a button that cannot work.
 *
 * The Tauri APIs are imported lazily so a browser build never pulls the
 * desktop chunk into the settings route.
 */
import { isDesktopRuntime } from '@/lib/runtime'

export interface AboutInfo {
  name: string
  version: string
  identifier: string
  tauri_version: string
  os: string
  arch: string
}

export interface UpdateRelease {
  version: string
  current_version: string
  notes: string | null
  pub_date: string | null
  /**
   * Updater target the release manifest is keyed by (`windows-x86_64`,
   * `darwin-aarch64`, ...) — the package this machine will actually install.
   */
  target: string
}

export interface UpdateProgress {
  downloaded: number
  total: number | null
}

export type UpdateCheck =
  | { kind: 'unsupported' }
  | { kind: 'current' }
  | { kind: 'available'; release: UpdateRelease }
  | { kind: 'error'; message: string }

/**
 * Installing never reports success: the shell either relaunches into the new
 * version or hands off to the platform installer, and in both cases this
 * process is gone before a resolved promise could be delivered. Only failures
 * come back.
 */
export type InstallFailure =
  | { kind: 'unsupported' }
  | { kind: 'error'; message: string }

const PROGRESS_EVENT = 'app://update-progress'

/** Tauri rejects with the plain string the command returned. */
function messageOf(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return String(error)
}

export async function readAbout(): Promise<AboutInfo | null> {
  if (!isDesktopRuntime()) return null
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    return await invoke<AboutInfo>('app_about')
  } catch {
    return null
  }
}

export async function checkForUpdate(): Promise<UpdateCheck> {
  if (!isDesktopRuntime()) return { kind: 'unsupported' }
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const release = await invoke<UpdateRelease | null>('check_for_update')
    return release ? { kind: 'available', release } : { kind: 'current' }
  } catch (error) {
    return { kind: 'error', message: messageOf(error) }
  }
}

export async function installUpdate(): Promise<InstallFailure> {
  if (!isDesktopRuntime()) return { kind: 'unsupported' }
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('install_update')
    // Reached only if the shell somehow survived the handoff; treat it as a
    // failure rather than leaving the UI stuck on "installing".
    return { kind: 'error', message: 'The installer exited without restarting the app.' }
  } catch (error) {
    return { kind: 'error', message: messageOf(error) }
  }
}

/** Resolves to an unsubscribe function; a no-op outside the desktop shell. */
export async function onUpdateProgress(
  handler: (progress: UpdateProgress) => void,
): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {}
  try {
    const { listen } = await import('@tauri-apps/api/event')
    return await listen<UpdateProgress>(PROGRESS_EVENT, (event) => handler(event.payload))
  } catch {
    return () => {}
  }
}

export function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`
}
