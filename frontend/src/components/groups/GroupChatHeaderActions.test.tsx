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

const prefetchConversation = vi.fn()
const idle = { mutateAsync: vi.fn(), isPending: false, error: null, reset: vi.fn() }

vi.mock('@/hooks/useGroupThreads', () => ({
  useCreateGroupThread: () => idle,
  useArchiveGroupThread: () => idle,
  useRestoreGroupThread: () => idle,
  useDeleteGroupThread: () => idle,
  useRenameGroupThread: () => idle,
}))
vi.mock('@/hooks/useGroupMessages', () => ({
  useClearGroupThreadMessages: () => idle,
  useConversationPrefetch: () => prefetchConversation,
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

function renderSwitcher(options: { tasks?: GroupThread[]; selected?: GroupThread; onSelect?: (id: string) => void; onDeleted?: (id: string) => void } = {}) {
  return render(
    <GroupChatHeaderActions
      groupId="group-1"
      threads={options.tasks ?? threads}
      selectedThread={options.selected ?? threads[0]}
      onSelect={options.onSelect ?? vi.fn()}
      onArchived={vi.fn()}
      onDeleted={options.onDeleted ?? vi.fn()}
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
  await user.click(screen.getByRole('button', { name: 'Current task' }))
}

describe('GroupChatHeaderActions task status', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    idle.mutateAsync.mockReset().mockResolvedValue({})
    prefetchConversation.mockReset()
    useConversationActivityStore.setState(initialActivity, true)
  })

  afterEach(() => {
    cleanup()
  })

  it('mirrors the selected task status onto the switcher trigger', () => {
    startRun('run-1', 'thread-1')

    renderSwitcher()

    expect(screen.getByRole('button', { name: 'Current task' })).toHaveTextContent(
      'Replying',
    )
  })

  it('gives every task its own status in the list', async () => {
    startRun('run-1', 'thread-1')
    startRun('run-2', 'thread-2')
    useConversationActivityStore.getState().markRunWaiting('run-2')

    await openSwitcher()

    const options = within(screen.getByRole('group', { name: 'Active' })).getAllByRole('button').filter((button) => button.hasAttribute('aria-pressed'))
    const names = options.map((option) => option.textContent)
    expect(names).toEqual(['ReplyingShip the API', 'Waiting for youWrite the docs'])
  })

  it('labels a dispatched task with the topics it follows', async () => {
    const dispatched = { ...thread('thread-3', 'Fix PR 7'), origin_thread_id: 'thread-1', topics: ['github:owner/repo#7'] }
    renderSwitcher({ tasks: [...threads, dispatched] })
    await userEvent.setup().click(screen.getByRole('button', { name: 'Current task' }))

    const options = within(screen.getByRole('group', { name: 'Active' })).getAllByRole('button').filter((button) => button.hasAttribute('aria-pressed'))
    expect(options.at(-1)).toHaveTextContent('Fix PR 7 · Dispatched · Following github:owner/repo#7')
  })

  it('says nothing about a task that is not doing anything', async () => {
    await openSwitcher()

    const options = within(screen.getByRole('group', { name: 'Active' })).getAllByRole('button').filter((button) => button.hasAttribute('aria-pressed'))
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

  it('renames an archived row without selecting it and trims the title', async () => {
    const user = userEvent.setup()
    const archived = { ...thread('old', 'Old task'), status: 'archived' }
    const onSelect = vi.fn()
    renderSwitcher({ tasks: [...threads, archived], onSelect })
    await user.click(screen.getByRole('button', { name: 'Current task' }))
    await user.click(screen.getByRole('button', { name: 'Rename “Old task”' }))
    const field = screen.getByRole('textbox', { name: 'Task title' })
    expect(field).toHaveValue('Old task')
    await user.clear(field)
    await user.type(field, '   ')
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
    await user.clear(field)
    await user.type(field, '  Better title  ')
    await user.keyboard('{Enter}')
    expect(idle.mutateAsync).toHaveBeenCalledWith({ threadId: 'old', title: 'Better title' })
    expect(onSelect).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('preserves the entered name on failure so it can be retried', async () => {
    const user = userEvent.setup()
    idle.mutateAsync.mockRejectedValueOnce(new Error('Offline'))
    renderSwitcher()
    await user.click(screen.getByRole('button', { name: 'Rename task' }))
    const field = screen.getByRole('textbox', { name: 'Task title' })
    await user.clear(field)
    await user.type(field, 'New name{Enter}')
    expect(field).toHaveValue('New name')
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(idle.mutateAsync).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('deletes an archived row after confirmation without switching the current task', async () => {
    const user = userEvent.setup()
    const archived = { ...thread('old', 'Old task'), status: 'archived' }
    const onSelect = vi.fn()
    const onDeleted = vi.fn()
    renderSwitcher({ tasks: [...threads, archived], onSelect, onDeleted })
    await user.click(screen.getByRole('button', { name: 'Current task' }))
    await user.click(screen.getByRole('button', { name: 'Delete “Old task”' }))
    expect(idle.mutateAsync).not.toHaveBeenCalled()
    const dialog = screen.getByRole('alertdialog')
    expect(dialog).toHaveTextContent('Delete “Old task”?')
    await user.click(within(dialog).getByRole('button', { name: 'Delete task' }))
    expect(idle.mutateAsync).toHaveBeenCalledWith('old')
    expect(onSelect).not.toHaveBeenCalled()
    expect(onDeleted).not.toHaveBeenCalled()
  })

  it('deletes only archived tasks, keeps the current task last, and retries unfinished targets', async () => {
    const user = userEvent.setup()
    const first = { ...thread('old-1', 'First old task'), status: 'archived' }
    const second = { ...thread('old-2', 'Second old task'), status: 'archived' }
    const selected = { ...thread('selected', 'Selected old task'), status: 'archived' }
    const onDeleted = vi.fn()
    idle.mutateAsync.mockResolvedValueOnce({}).mockRejectedValueOnce(new Error('Offline'))
    renderSwitcher({ tasks: [selected, ...threads, first, second], selected, onDeleted })
    await user.click(screen.getByRole('button', { name: 'Current task' }))
    await user.click(screen.getByRole('button', { name: 'Delete all archived' }))
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Delete 3 archived tasks?')
    await user.click(screen.getByRole('button', { name: 'Delete all archived' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Offline')
    expect(onDeleted).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Delete all archived' }))
    expect(idle.mutateAsync.mock.calls.map(([id]) => id)).toEqual(['old-1', 'old-2', 'old-2', 'selected'])
    expect(onDeleted).toHaveBeenCalledExactlyOnceWith('selected', ['old-2', 'selected'])
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })

  it('allows keyboard task selection and leaves cancellation without mutations', async () => {
    const user = userEvent.setup()
    const onSelect = vi.fn()
    renderSwitcher({ onSelect })
    const trigger = screen.getByRole('button', { name: 'Current task' })
    trigger.focus()
    await user.keyboard('{Enter}')
    const option = screen.getByRole('button', { name: 'Write the docs' })
    option.focus()
    await user.keyboard('{Enter}')
    expect(onSelect).toHaveBeenCalledWith('thread-2')
    await user.click(screen.getByRole('button', { name: 'Rename task' }))
    await user.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(idle.mutateAsync).not.toHaveBeenCalled()
  })
})
