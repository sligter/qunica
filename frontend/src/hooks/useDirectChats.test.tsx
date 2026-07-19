import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import type { PropsWithChildren } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  directChatQueryKey,
  directChatsQueryKey,
  useCreateDirectChat,
  useDeleteDirectChat,
  useRenameDirectChat,
} from './useDirectChats'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { DirectChatRead } from '@/types/api'

vi.mock('@/lib/api-v2/client', () => ({ fetchJson: vi.fn() }))

const chat: DirectChatRead = {
  id: 'chat-1',
  title: 'Original',
  title_source: 'automatic',
  agent_id: 'agent-1',
  agent_name: 'Solo',
  agent_status: 'active',
  workspace_id: 'workspace-1',
  status: 'active',
  created_at: '2026-07-19T00:00:00Z',
  updated_at: '2026-07-19T00:00:00Z',
}

function createWrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

describe('useDirectChats mutations', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'test-token' })
  })

  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
    useAuthStore.setState({ token: null })
  })

  it('posts creates, seeds detail, and inserts the activity-sorted list entry', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const newer = { ...chat, id: 'chat-2', updated_at: '2026-07-20T00:00:00Z' }
    vi.mocked(fetchJson).mockResolvedValueOnce(newer)
    client.setQueryData(directChatsQueryKey, [chat])
    const { result } = renderHook(() => useCreateDirectChat(), { wrapper: createWrapper(client) })

    await act(async () => {
      await result.current.mutateAsync({ agent_id: 'agent-1' })
    })

    expect(fetchJson).toHaveBeenCalledWith('/direct-chats', {
      method: 'POST',
      body: { agent_id: 'agent-1' },
      token: 'test-token',
    })
    expect(client.getQueryData<DirectChatRead[]>(directChatsQueryKey)?.map((item) => item.id)).toEqual([
      'chat-2',
      'chat-1',
    ])
    expect(client.getQueryData(directChatQueryKey('chat-2'))).toEqual(newer)
  })

  it('optimistically renames both caches and restores them after failure', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    let rejectRequest: (reason?: unknown) => void = () => undefined
    vi.mocked(fetchJson).mockReturnValueOnce(
      new Promise<DirectChatRead>((_resolve, reject) => {
        rejectRequest = reject
      }),
    )
    client.setQueryData(directChatsQueryKey, [chat])
    client.setQueryData(directChatQueryKey(chat.id), chat)
    const { result } = renderHook(() => useRenameDirectChat(chat.id), {
      wrapper: createWrapper(client),
    })

    act(() => result.current.mutate({ title: 'Renamed' }))

    await waitFor(() => {
      expect(client.getQueryData<DirectChatRead>(directChatQueryKey(chat.id))?.title).toBe('Renamed')
      expect(client.getQueryData<DirectChatRead[]>(directChatsQueryKey)?.[0]?.title).toBe('Renamed')
    })
    act(() => rejectRequest(new Error('save failed')))
    await waitFor(() => {
      expect(client.getQueryData(directChatQueryKey(chat.id))).toEqual(chat)
      expect(client.getQueryData(directChatsQueryKey)).toEqual([chat])
    })
  })

  it('uses PATCH for rename and DELETE for deletion', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    client.setQueryData(directChatsQueryKey, [chat])
    client.setQueryData(directChatQueryKey(chat.id), chat)
    vi.mocked(fetchJson).mockResolvedValueOnce({ ...chat, title: 'Renamed', title_source: 'manual' })
    const rename = renderHook(() => useRenameDirectChat(chat.id), { wrapper: createWrapper(client) })
    await act(async () => {
      await rename.result.current.mutateAsync({ title: 'Renamed' })
    })
    expect(fetchJson).toHaveBeenLastCalledWith(`/direct-chats/${chat.id}`, {
      method: 'PATCH',
      body: { title: 'Renamed' },
      token: 'test-token',
    })

    vi.mocked(fetchJson).mockResolvedValueOnce(undefined)
    const remove = renderHook(() => useDeleteDirectChat(chat.id), { wrapper: createWrapper(client) })
    await act(async () => {
      await remove.result.current.mutateAsync()
    })
    expect(fetchJson).toHaveBeenLastCalledWith(`/direct-chats/${chat.id}`, {
      method: 'DELETE',
      token: 'test-token',
    })
    expect(client.getQueryData(directChatsQueryKey)).toEqual([])
    expect(client.getQueryData(directChatQueryKey(chat.id))).toBeUndefined()
  })
})
