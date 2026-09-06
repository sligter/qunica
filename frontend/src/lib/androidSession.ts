import { invoke } from '@tauri-apps/api/core'
import { create } from 'zustand'

export const isAndroidRuntime = () => import.meta.env.MODE === 'android'

export function normalizeServerOrigin(value: string): string {
  const url = new URL(value.trim())
  if (url.protocol !== 'https:' || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error('Use an HTTPS server address without a path, credentials, query or fragment.')
  }
  if (['tauri.localhost', 'ipc.localhost'].includes(url.hostname)) throw new Error('Choose your Qunica Server address.')
  return url.origin
}

export interface LanConnection { endpoint: string; publicKey: string; credential: string }
export const LAN_ORIGIN = 'https://qunica-lan.invalid'
interface Session { server: string | null; token: string | null; lan?: LanConnection }
interface AndroidState { server: string | null; ready: boolean; error: string | null }
export const useAndroidSession = create<AndroidState>(() => ({ server: null, ready: false, error: null }))
let session: Session = { server: null, token: null }
let writes: Promise<void> = Promise.resolve()
let initialization: Promise<string | null> | undefined

/** No browser storage fallback: credentials belong to Android Keystore. */
export function initializeAndroidSession(): Promise<string | null> {
  if (!initialization) initialization = invoke<{ value: string | null }>('mobile_session_read').then(async ({ value }) => {
    const parsed = value ? JSON.parse(value) as Session : { server: null, token: null }
    if (parsed.lan) await invoke('mobile_lan_configure', { connection: parsed.lan })
    const server = parsed.lan ? LAN_ORIGIN : parsed.server ? normalizeServerOrigin(parsed.server) : null
    if (server === LAN_ORIGIN && !parsed.lan) throw new Error('Missing desktop pairing. Scan again.')
    session = { server, token: server && typeof parsed.token === 'string' ? parsed.token : null, ...(parsed.lan ? { lan: parsed.lan } : {}) }
    localStorage.removeItem('qunica:auth:v1')
    useAndroidSession.setState({ server, ready: true, error: null })
    return session.token
  }).catch(error => {
    initialization = undefined
    useAndroidSession.setState({ error: String(error) })
    throw error
  })
  return initialization
}

function persist(next: Session): Promise<void> {
  session = next
  const operation = writes.then(async () => {
    await invoke('mobile_session_write', { value: JSON.stringify(next) })
  })
  writes = operation.catch(error => { useAndroidSession.setState({ error: String(error) }) })
  return operation.then(() => { useAndroidSession.setState({ error: null }) })
}

export function saveAndroidToken(token: string | null): Promise<void> {
  if (token && !session.server) return Promise.reject(new Error('Configure a server before signing in.'))
  return persist({ ...session, token })
}

export async function changeAndroidServer(value: string): Promise<void> {
  const server = normalizeServerOrigin(value)
  await persist({ server, token: null })
  if (useAndroidSession.getState().server === LAN_ORIGIN) await invoke('mobile_lan_configure', { connection: null })
  useAndroidSession.setState({ server })
}

export async function pairAndroidDesktop(offer: string): Promise<void> {
  const lan = await invoke<LanConnection>('mobile_lan_pair', { offer, name: 'Android' })
  await persist({ server: LAN_ORIGIN, token: null, lan })
  await invoke('mobile_lan_configure', { connection: lan })
  useAndroidSession.setState({ server: LAN_ORIGIN })
}

export const androidDesktopAddress = () => session.lan?.endpoint ?? session.server

export const retryAndroidPersistence = () => persist(session)
