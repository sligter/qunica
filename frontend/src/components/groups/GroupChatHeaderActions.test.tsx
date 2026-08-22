import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { GroupChatHeaderActions } from '@/components/groups/GroupChatHeaderActions'
import i18n from '@/i18n'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'
import type { GroupThread } from '@/types/api'

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.stubGlobal('ResizeObserver', ResizeObserverMock)
Element.prototype.hasPointerCapture = () => false
Element.prototype.setPointerCapture = () => {}
Element.prototype.releasePointerCapture = () => {}
Element.prototype.scrollIntoView = () => {}

const idle = { mutateAsync: vi.fn(), isPending: false, error: null, reset: vi.fn() }

vi.mock('@/hooks/useGroupThreads', () => ({
  useCreateGroupThread: () => idle,
  useArchiveGroupThread: () => idle,
  useRestoreGroupThread: () => idle,
  useDeleteGroupThread: () => idle,
}))
vi.mock('@/hooks/useGroupMessages', () => ({
  useClearGroupThreadMessages: () => idle,
}))
vi.mock('@/hooks/useWorkspaceGit', () => ({
  useGroupWorkspaceGitBranches: () => ({ data: undefined, isLoading: false, error: null }),
}))

const initialActivity = useConversationActivityStore.getInitialState()

function thread(id: string, title: string): GroupThread {
  return {
    id,
    group_id: 'group-1',
    agent_id: null,
    created_by: null,
    thread_type: 'task_thread',
    title,
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
}

const threads = [thread('thread-1', 'Ship the API'), thread('thread-2', 'Write the docs')]

function renderSwitcher() {
  return render(
    <GroupChatHeaderActions
      groupId="group-1"
      threads={threads}
      selectedThread={threads[0]}
      onSelect={vi.fn()}
      onArchived={vi.fn()}
      onDeleted={vi.fn()}
    />,
  )
}

function startRun(id: string, threadId: string) {
  useConversationActivityStore.getState().startRun({
    id,
    conversationId: 'group-1',
    threadId,
    scope: 'groups',
  })
}

async function openSwitcher() {
  const user = userEvent.setup()
  renderSwitcher()
  await user.click(screen.getByRole('combobox', { name: 'Current task' }))
}

describe('GroupChatHeaderActions task status', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    idle.mutateAsync.mockReset().mockResolvedValue({})
    useConversationActivityStore.setState(initialActivity, true)
  })

  afterEach(() => {
    cleanup()
  })

  it('mirrors the selected task status onto the switcher trigger', () => {
    startRun('run-1', 'thread-1')

    renderSwitcher()

    expect(screen.getByRole('combobox', { name: 'Current task' })).toHaveTextContent(
      'Replying',
    )
  })

  it('gives every task its own status in the list', async () => {
    startRun('run-1', 'thread-1')
    startRun('run-2', 'thread-2')
    useConversationActivityStore.getState().markRunWaiting('run-2')

    await openSwitcher()

    const options = await screen.findAllByRole('option')
    const names = options.map((option) => option.textContent)
    expect(names).toEqual(['ReplyingShip the API', 'Waiting for youWrite the docs'])
  })

  it('says nothing about a task that is not doing anything', async () => {
    await openSwitcher()

    const options = await screen.findAllByRole('option')
    expect(options.map((option) => option.textContent)).toEqual([
      'Ship the API',
      'Write the docs',
    ])
  })

  it('clears only the selected task from the header action', async () => {
    const user = userEvent.setup()
    renderSwitcher()

    await user.click(screen.getByRole('button', { name: 'Clear current task' }))
    const dialog = screen.getByRole('alertdialog')
    await user.click(within(dialog).getByRole('button', { name: 'Clear current task' }))

    expect(idle.mutateAsync).toHaveBeenCalledWith('thread-1')
  })
})
