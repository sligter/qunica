import { afterEach, expect, it, vi } from 'vitest'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import { abortOnAuthChange, authFetch, signOut } from './authFetch'

afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); useAuthStore.getState().logout() })

it('never sends a previous instance credential to a newly selected origin', async () => {
  vi.stubEnv('VITE_API_BASE_URL','https://first-instance.example')
  useAuthStore.getState().setToken('instance-bound-token')
  const fetch=vi.fn()
  vi.stubGlobal('fetch',fetch)
  useAuthStore.getState().logout()
  vi.stubEnv('VITE_API_BASE_URL','https://second-instance.example')
  await expect(authFetch('https://second-instance.example/api/v2/auth/me',{headers:{Authorization:'Bearer instance-bound-token'}})).rejects.toMatchObject({name:'AbortError'})
  expect(fetch).not.toHaveBeenCalled()
})

it('ignores a stale 401, but expires the current session and aborts its streams', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 401 })))
  useAuthStore.getState().setToken('new-token')
  const controller = new AbortController()
  abortOnAuthChange(controller, 'new-token')
  await authFetch('/api/v2/auth/me', { headers: { Authorization: 'Bearer old-token' } })
  expect(useAuthStore.getState().token).toBe('new-token')
  expect(controller.signal.aborted).toBe(false)
  useMessageStore.setState({ byGroup: { private: [] } })
  await authFetch('/api/v2/auth/me', { headers: { Authorization: 'Bearer new-token' } })
  expect(useAuthStore.getState().token).toBeNull()
  expect(controller.signal.aborted).toBe(true)
  expect(useMessageStore.getState().byGroup).toEqual({})
})

it('clears a legacy login and private state, while preserving the migration prompt', async () => {
  useAuthStore.getState().setToken('legacy-token')
  useMessageStore.setState({ byGroup: { private: [] } })
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({
    error: { code: 'session_upgrade_required', message: 'Sign in again' },
  }), { status: 401 })))
  const response = await authFetch('/api/v2/auth/me', { headers: { Authorization: 'Bearer legacy-token' } })
  expect((await response.json()).error.code).toBe('session_upgrade_required')
  expect(useAuthStore.getState().token).toBeNull()
  expect(useAuthStore.getState().loginNotice).toBe('session_upgrade_required')
  expect(useMessageStore.getState().byGroup).toEqual({})
  useAuthStore.getState().setToken('new-session')
  expect(useAuthStore.getState().loginNotice).toBeNull()
})

it('revokes the device session before clearing its saved login', async () => {
  useAuthStore.getState().setToken('current-session')
  const fetch = vi.fn().mockImplementation(async () => {
    expect(useAuthStore.getState().token).toBe('current-session')
    return new Response(null, { status: 204 })
  })
  vi.stubGlobal('fetch', fetch)
  expect(await signOut()).toBe(true)
  expect(fetch).toHaveBeenCalledWith('/api/v2/auth/logout', expect.objectContaining({
    method: 'POST', headers: { Authorization: 'Bearer current-session' },
  }))
  expect(useAuthStore.getState().token).toBeNull()
})

it('keeps the login available when server revocation fails', async () => {
  useAuthStore.getState().setToken('current-session')
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 503 })))
  await expect(signOut()).rejects.toThrow('Unable to revoke device session')
  expect(useAuthStore.getState().token).toBe('current-session')
})

it('does not clear a new account when a previous logout finishes late', async () => {
  useAuthStore.getState().setToken('old-session')
  vi.stubGlobal('fetch', vi.fn().mockImplementation(async () => {
    useAuthStore.getState().setToken('new-session')
    return new Response(null, { status: 204 })
  }))
  expect(await signOut()).toBe(false)
  expect(useAuthStore.getState().token).toBe('new-session')
})
