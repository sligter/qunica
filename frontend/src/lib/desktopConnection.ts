import { create } from 'zustand'
import { normalizeServerOrigin } from './androidSession'

const KEY = 'qunica:desktop-connection:v1'
const INSTANCES_KEY = 'qunica:desktop-instances:v1'
export type DesktopConnection = { mode: 'local' } | { mode: 'remote'; origin: string }
export function desktopConnectionId(connection: DesktopConnection): string {
  return connection.mode === 'local' ? 'local' : connection.origin
}

export function readDesktopInstances(): DesktopConnection[] {
  const instances: DesktopConnection[] = [{ mode: 'local' }]
  let saved: unknown
  try { saved = JSON.parse(localStorage.getItem(INSTANCES_KEY) ?? '[]') } catch { /* Ignore an invalid history. */ }
  for (const item of [...(Array.isArray(saved) ? saved : []), load()]) {
    try {
      if (item?.mode !== 'remote' || typeof item.origin !== 'string') continue
      const origin = normalizeServerOrigin(item.origin)
      if (!instances.some(instance => desktopConnectionId(instance) === origin)) {
        instances.push({ mode: 'remote', origin })
      }
    } catch { /* Invalid saved addresses must never become credential destinations. */ }
  }
  return instances
}
export function hasDesktopConnection(): boolean {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? 'null') as DesktopConnection | null
    if (value?.mode === 'local') return true
    return value?.mode === 'remote' && !!normalizeServerOrigin(value.origin)
  } catch { return false }
}
function load(): DesktopConnection {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? 'null') as DesktopConnection | null
    if (value?.mode === 'remote') return { mode: 'remote', origin: normalizeServerOrigin(value.origin) }
  } catch { /* An invalid setting must never become a credential destination. */ }
  return { mode: 'local' }
}
export const useDesktopConnection = create<{ connection: DesktopConnection; instances: DesktopConnection[] }>(() => ({ connection: load(), instances: readDesktopInstances() }))

export function saveDesktopConnection(connection: DesktopConnection): void {
  const normalized: DesktopConnection = connection.mode === 'remote'
    ? { mode: 'remote', origin: normalizeServerOrigin(connection.origin) } : connection
  const instances = readDesktopInstances()
  if (!instances.some(instance => desktopConnectionId(instance) === desktopConnectionId(normalized))) {
    instances.push(normalized)
  }
  localStorage.setItem(KEY, JSON.stringify(normalized))
  localStorage.setItem(INSTANCES_KEY, JSON.stringify(instances))
  useDesktopConnection.setState({ connection: normalized, instances })
}

if (typeof window !== 'undefined') {
  const changed = (event: StorageEvent) => {
    if (event.key === KEY) window.location.replace('/')
    else if (event.key === INSTANCES_KEY) useDesktopConnection.setState({ instances: readDesktopInstances() })
  }
  window.addEventListener('storage', changed)
  import.meta.hot?.dispose(() => window.removeEventListener('storage', changed))
}
