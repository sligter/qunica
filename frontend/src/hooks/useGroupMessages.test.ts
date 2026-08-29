import { QueryClient } from '@tanstack/react-query'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { fetchJson } from '@/lib/api-v2/client'
import { conversationMessagesKey, prefetchConversation } from './useGroupMessages'
import type { GroupThread } from '@/types/api'

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
