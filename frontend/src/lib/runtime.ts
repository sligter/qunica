import { isAndroidRuntime, useAndroidSession } from './androidSession'
import { useDesktopConnection } from './desktopConnection'

const DESKTOP_API_ORIGIN = 'http://127.0.0.1:8765'

export function isDesktopRuntime(): boolean {
  if (typeof window === 'undefined' || isAndroidRuntime()) {
    return false
  }
  return window.location.hostname === 'tauri.localhost' || '__TAURI_INTERNALS__' in window
}

export function apiOrigin(): string {
  if (isAndroidRuntime()) {
    const server = useAndroidSession.getState().server
    if (!server) throw new Error('Configure a Qunica Server before using the Android client.')
    return server
  }
  if (isDesktopRuntime()) {
    const connection = useDesktopConnection.getState().connection
    return connection.mode === 'remote' ? connection.origin : DESKTOP_API_ORIGIN
  }
  const configured = import.meta.env.VITE_API_BASE_URL
  if (typeof configured === 'string' && configured.trim()) {
    return configured.replace(/\/+$/, '')
  }
  return ''
}

export function usesLocalDesktop(): boolean {
  return isDesktopRuntime() && useDesktopConnection.getState().connection.mode === 'local'
}

export function apiUrl(path: string): string {
  const normalized = path.startsWith('/') ? path : `/${path}`
  return `${apiOrigin()}${normalized}`
}
