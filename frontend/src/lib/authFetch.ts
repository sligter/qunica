import { useAuthStore } from '@/stores/authStore'

/** A late 401 for an old token must not sign out a newly signed-in account. */
export function expireAuthToken(token: string | null | undefined): void {
  const auth = useAuthStore.getState()
  if (token && auth.token === token) auth.logout()
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
  const response = await fetch(input, { ...init, cache: 'no-store' })
  if (response.status === 401) {
    const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined))
    const authorization = headers.get('authorization')
    if (authorization?.startsWith('Bearer ')) expireAuthToken(authorization.slice(7))
  }
  return response
}
