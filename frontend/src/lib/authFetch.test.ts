import { afterEach, expect, it, vi } from 'vitest'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import { abortOnAuthChange, authFetch } from './authFetch'

afterEach(() => { vi.unstubAllGlobals(); useAuthStore.getState().logout() })

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
