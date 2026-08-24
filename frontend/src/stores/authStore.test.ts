import { beforeEach, describe, expect, it } from 'vitest'

import { queryClient } from '@/lib/queryClient'
import { useAuthStore } from '@/stores/authStore'
import { useQueuedMessagesStore } from '@/stores/queuedMessagesStore'

const STORAGE_KEY = 'agentchat:auth:v1'

describe('authStore window synchronization', () => {
  beforeEach(() => {
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
