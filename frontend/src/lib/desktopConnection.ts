import { create } from 'zustand'
import { normalizeServerOrigin } from './androidSession'

const KEY = 'qunica:desktop-connection:v1'
export type DesktopConnection = { mode: 'local' } | { mode: 'remote'; origin: string }
function load(): DesktopConnection {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? 'null') as DesktopConnection | null
    if (value?.mode === 'remote') return { mode: 'remote', origin: normalizeServerOrigin(value.origin) }
  } catch { /* An invalid setting must never become a credential destination. */ }
  return { mode: 'local' }
}
export const useDesktopConnection = create<{ connection: DesktopConnection }>(() => ({ connection: load() }))

export function saveDesktopConnection(connection: DesktopConnection): void {
  const normalized = connection.mode === 'remote'
    ? { mode: 'remote', origin: normalizeServerOrigin(connection.origin) } : connection
  localStorage.setItem(KEY, JSON.stringify(normalized))
  useDesktopConnection.setState({ connection: normalized as DesktopConnection })
}

if (typeof window !== 'undefined') {
  const changed = (event: StorageEvent) => { if (event.key === KEY) window.location.replace('/') }
  window.addEventListener('storage', changed)
  import.meta.hot?.dispose(() => window.removeEventListener('storage', changed))
}
