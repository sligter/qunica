import { REALTIME_PROTOCOL } from './client'

export async function checkInstance(origin: string): Promise<void> {
  const response = await fetch(`${origin}/api/v2/instance`, { signal: AbortSignal.timeout(10_000), cache: 'no-store', credentials: 'omit' })
  if (!response.ok) throw new Error(`Unable to connect to server (HTTP ${response.status})`)
  const instance = await response.json() as { protocol_versions?: string[] }
  if (!instance.protocol_versions?.includes(REALTIME_PROTOCOL)) throw new Error('This server needs an upgrade before this client can connect.')
}
