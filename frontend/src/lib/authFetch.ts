import { authTokenOrigin, useAuthStore } from '@/stores/authStore'
import { isAndroidRuntime, LAN_ORIGIN, useAndroidSession } from './androidSession'
import { apiUrl } from './runtime'

/** A late 401 for an old token must not sign out a newly signed-in account. */
export function expireAuthToken(
  token: string | null | undefined,
  notice: 'session_upgrade_required' | 'session_revoked' | null = null,
): void {
  const auth = useAuthStore.getState()
  if (token && auth.token === token) auth.logout(notice)
}

/** Clear local credentials only after the captured server confirms revocation. */
export async function signOut(): Promise<boolean> {
  const token = useAuthStore.getState().token
  if (!token) { useAuthStore.getState().logout(); return true }
  const response = await authFetch(apiUrl('/api/v2/auth/logout'), {
    method: 'POST', headers: { Authorization: `Bearer ${token}` },
  })
  if (!response.ok && response.status !== 401) throw new Error('Unable to revoke device session')
  const current = useAuthStore.getState().token
  if (current && current !== token) return false
  expireAuthToken(token)
  return true
}

/** Bind streaming body lifetime, not just the fetch response headers, to login. */
export function abortOnAuthChange(controller: AbortController, token: string): () => void {
  const unsubscribe = useAuthStore.subscribe(state => {
    if (state.token !== token) controller.abort()
  })
  const cleanup = () => {
    unsubscribe()
    controller.signal.removeEventListener('abort', cleanup)
  }
  controller.signal.addEventListener('abort', cleanup, { once: true })
  return cleanup
}

export async function authFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined))
  const bearer = headers.get('authorization')
  if (bearer?.startsWith('Bearer ')) {
    const origin = authTokenOrigin(bearer.slice(7))
    const destination = new URL(input instanceof Request ? input.url : String(input), window.location.href).origin
    // Note: queued callbacks can run between instance selection and navigation.
    // Their old credentials may still target the old server, never the new one.
    if (origin && origin !== destination) throw new DOMException('Credential belongs to another server', 'AbortError')
  }
  const response = isAndroidRuntime() && useAndroidSession.getState().server === LAN_ORIGIN
    ? await (await import('./lanFetch')).lanFetch(input, init)
    : await fetch(input, { ...init, cache: 'no-store' })
  if (response.status === 401) {
    const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined))
    const authorization = headers.get('authorization')
    if (authorization?.startsWith('Bearer ')) {
      const code = await response.clone().json().then(body => body?.error?.code as unknown).catch(() => null)
      expireAuthToken(authorization.slice(7),
        code === 'session_upgrade_required' || code === 'session_revoked' ? code : null)
    }
  }
  return response
}
