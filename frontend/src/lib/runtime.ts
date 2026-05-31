const DESKTOP_API_ORIGIN = 'http://127.0.0.1:8765'

export function apiOrigin(): string {
  const configured = import.meta.env.VITE_API_BASE_URL
  if (typeof configured === 'string' && configured.trim()) {
    return configured.replace(/\/+$/, '')
  }
  if (window.location.hostname === 'tauri.localhost') {
    return DESKTOP_API_ORIGIN
  }
  return ''
}

export function apiUrl(path: string): string {
  const normalized = path.startsWith('/') ? path : `/${path}`
  return `${apiOrigin()}${normalized}`
}
