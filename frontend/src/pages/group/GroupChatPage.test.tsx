import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'

import { GroupChatPage } from '@/pages/group/GroupChatPage'
import i18n from '@/i18n'
import type { GroupThread } from '@/types/api'

const mainThread: GroupThread = {
  id: 'main-1',
  group_id: 'group-1',
  agent_id: null,
  created_by: null,
  thread_type: 'task_thread',
  title: 'Main conversation',
  git_branch: null,
  worktree_path: null,
  goal: null,
  status: 'active',
  priority: 0,
  started_at: null,
  completed_at: null,
  created_at: '2026-07-22T00:00:00Z',
  updated_at: '2026-07-22T00:00:00Z',
}

const dispatchedThread: GroupThread = {
  ...mainThread,
  id: 'task-1',
  title: 'Dispatched task',
  origin_thread_id: mainThread.id,
}

vi.mock('@/hooks/useGroups', () => ({
  useGroup: () => ({
    data: { id: 'group-1', name: 'Group', workspace_id: null, moderator_enabled: false, announcement: null },
    error: null,
    isLoading: false,
  }),
}))

vi.mock('@/hooks/useGroupThreads', () => ({
  useGroupThreads: () => ({ data: [mainThread, dispatchedThread], error: null, isLoading: false }),
}))

vi.mock('@/hooks/useGroupAgents', () => ({
  useGroupAgents: () => ({ data: [] }),
}))

vi.mock('@/components/groups/GroupChatHeaderActions', () => ({
  GroupChatHeaderActions: () => <span data-testid="task-actions" />,
}))

vi.mock('@/components/chat/ConversationChatView', () => ({
  ConversationChatView: ({
    threadId,
    renderHeaderContext,
  }: {
    threadId?: string
    renderHeaderContext?: (disabled: boolean) => ReactNode
  }) => (
    <div>
      <span data-testid="thread-id">{threadId}</span>
      {renderHeaderContext?.(false)}
    </div>
  ),
}))

function renderPage() {
  return render(
    <MemoryRouter initialEntries={['/groups/group-1']}>
      <Routes>
        <Route path="/groups/:groupId" element={<GroupChatPage />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('GroupChatPage task source navigation', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    window.localStorage.clear()
    window.localStorage.setItem('qunica:groups:selected-thread:group-1', dispatchedThread.id)
  })

  afterEach(() => {
    cleanup()
    window.localStorage.clear()
  })

  it('returns a dispatched task to its origin thread', async () => {
    const user = userEvent.setup()
    renderPage()

    await waitFor(() => expect(screen.getByTestId('thread-id')).toHaveTextContent('task-1'))
    await user.click(screen.getByRole('button', { name: 'Back to source conversation' }))

    expect(screen.getByTestId('thread-id')).toHaveTextContent('main-1')
    expect(screen.queryByRole('button', { name: 'Back to source conversation' })).not.toBeInTheDocument()
  })
})
