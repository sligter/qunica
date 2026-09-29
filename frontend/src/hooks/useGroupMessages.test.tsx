import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { fetchJson } from '@/lib/api-v2/client'
import {
  conversationMessagesKey,
  prefetchConversation,
  useConversationMessages,
} from './useGroupMessages'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { GroupThread, Message } from '@/types/api'

vi.mock('@/lib/api-v2/client', () => ({ fetchJson: vi.fn() }))

const thread = (id: string): GroupThread => ({
  id,
  group_id: 'group-1',
  agent_id: null,
  created_by: null,
  thread_type: 'task_thread',
  title: id,
  git_branch: null,
  worktree_path: null,
  goal: null,
  status: 'active',
  priority: 0,
  started_at: null,
  completed_at: null,
  created_at: '2026-08-29T00:00:00Z',
  updated_at: '2026-08-29T00:00:00Z',
})

afterEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
})

const initialMessageState = useMessageStore.getInitialState()

function storedMessage(id: string): Message {
  return {
    id,
    group_id: 'group-1',
    thread_id: 'thread-2',
    sender_type: 'agent',
    sender_id: 'agent-1',
    message_type: 'text',
    content: id,
    attachments: [],
    status: 'visible',
    refs: null,
    context_usage: null,
    turn_id: null,
    dispatch_id: null,
    reply_to_message_id: null,
    turn_summary: null,
    created_at: '2026-08-29T00:00:00Z',
  }
}

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

describe('conversation intent prefetch', () => {
  it('warms the last selected group task before navigation', async () => {
    localStorage.setItem('qunica:groups:selected-thread:group-1', 'thread-2')
    vi.mocked(fetchJson).mockImplementation(async (path) => {
      if (path === '/groups/group-1/threads') return [thread('thread-1'), thread('thread-2')] as never
      if (path.includes('/messages?')) return [] as never
      throw new Error(`Unexpected request: ${path}`)
    })
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })

    await prefetchConversation(queryClient, 'token', 'groups', 'group-1')

    expect(fetchJson).toHaveBeenNthCalledWith(1, '/groups/group-1/threads', { token: 'token' })
    expect(fetchJson).toHaveBeenNthCalledWith(
      2,
      '/groups/group-1/messages?limit=30&thread_id=thread-2',
      { token: 'token' },
    )
    expect(queryClient.getQueryData(
      conversationMessagesKey('groups', 'group-1', 'thread-2'),
    )).toEqual({ pages: [[]], pageParams: [undefined] })
  })
})

describe('conversation history merge', () => {
  beforeEach(() => {
    useMessageStore.setState(initialMessageState, true)
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
  })

  it('keeps message order when a conversation with a running reply is reopened', async () => {
    // The reply keeps streaming while the user is away, and coming back mounts
    // the query again (staleTime 0) against the newest-30 window.
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    vi.mocked(fetchJson).mockResolvedValueOnce(
      Array.from({ length: 30 }, (_, index) => storedMessage(`h${index + 1}`)) as never,
    )
    const first = renderHook(
      () => useConversationMessages('groups', 'group-1', 'thread-2'),
      { wrapper: wrapper(queryClient) },
    )
    await waitFor(() =>
      expect(useMessageStore.getState().byGroup['thread-2']).toHaveLength(30),
    )

    useMessageStore.getState().startSend('thread-2', async () => undefined)
    for (const id of ['n31', 'n32', 'n33']) {
      useMessageStore.getState().appendMessage('thread-2', storedMessage(id))
    }
    first.unmount()

    vi.mocked(fetchJson).mockResolvedValueOnce([
      ...Array.from({ length: 27 }, (_, index) => storedMessage(`h${index + 4}`)),
      storedMessage('n31'),
      storedMessage('n32'),
      storedMessage('n33'),
    ] as never)
    renderHook(() => useConversationMessages('groups', 'group-1', 'thread-2'), {
      wrapper: wrapper(queryClient),
    })
    await waitFor(() => expect(fetchJson).toHaveBeenCalledTimes(2))
    await waitFor(() =>
      expect(useMessageStore.getState().byGroup['thread-2']).toHaveLength(33),
    )

    expect(useMessageStore.getState().byGroup['thread-2'].map(({ id }) => id)).toEqual([
      ...Array.from({ length: 30 }, (_, index) => `h${index + 1}`),
      'n31',
      'n32',
      'n33',
    ])
  })
})
