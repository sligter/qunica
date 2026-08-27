import { beforeEach, describe, expect, it } from 'vitest'

import { queryClient } from '@/lib/queryClient'
import { useAuthStore } from '@/stores/authStore'
import { useQueuedMessagesStore } from '@/stores/queuedMessagesStore'

const STORAGE_KEY = 'qunica:auth:v1'
const USER_SYNC_KEY = 'qunica:auth-user:v1'

describe('authStore window synchronization', () => {
  beforeEach(() => {
    localStorage.clear()
    queryClient.clear()
    useAuthStore.setState({ token: null, user: null, hydrated: true })
    useQueuedMessagesStore.getState().clearAll()
  })

  it('signs this window out when another window removes the token', () => {
    useAuthStore.setState({ token: 'stale-token', hydrated: true })
    useQueuedMessagesStore.getState().enqueue('chat-1', [{
      content: 'private draft',
      attachments: [],
    }])
    queryClient.setQueryData(['private-data'], { owner: 'old-account' })

    window.dispatchEvent(new StorageEvent('storage', {
      key: STORAGE_KEY,
      newValue: null,
    }))

    expect(useAuthStore.getState()).toEqual(expect.objectContaining({
      token: null,
      user: null,
      hydrated: true,
    }))
    expect(useQueuedMessagesStore.getState().byStateId).toEqual({})
    expect(queryClient.getQueryData(['private-data'])).toBeUndefined()
  })

  it('hydrates a token written by another window through the normal auth check', () => {
    window.dispatchEvent(new StorageEvent('storage', {
      key: STORAGE_KEY,
      newValue: JSON.stringify({ token: 'fresh-token' }),
    }))

    expect(useAuthStore.getState()).toEqual(expect.objectContaining({
      token: 'fresh-token',
      user: null,
      hydrated: false,
    }))
  })

  it('applies a profile saved in another window immediately', () => {
    const current = {
      id: 'user-1',
      email: 'nova@example.com',
      name: 'Nova',
      avatar_url: null,
      created_at: '2026-01-01T00:00:00Z',
    }
    useAuthStore.setState({ token: 'token', user: current })
    const updated = { ...current, name: 'Nova Ray', avatar_url: 'preset:prism' }
    useAuthStore.getState().setUser(updated)
    const payload = localStorage.getItem(USER_SYNC_KEY)
    useAuthStore.setState({ user: current })

    window.dispatchEvent(new StorageEvent('storage', {
      key: USER_SYNC_KEY,
      newValue: payload,
    }))

    expect(useAuthStore.getState().user).toEqual(updated)
    expect(queryClient.getQueryData(['auth', 'me', 'token'])).toEqual(updated)
  })

  it('does not restore an in-flight queued message after logout clears it', () => {
    const queued = useQueuedMessagesStore.getState()
    const input = { content: 'old-account secret', attachments: [] }
    queued.enqueue('chat-1', [input])
    expect(queued.beginDispatch('chat-1')).toEqual(input)

    useAuthStore.setState({ token: 'old-token' })
    useAuthStore.getState().logout()
    useQueuedMessagesStore.getState().finishDispatch('chat-1', input)

    expect(useQueuedMessagesStore.getState().byStateId).toEqual({})
    expect(useQueuedMessagesStore.getState().dispatchingByStateId).toEqual({})
  })
})
