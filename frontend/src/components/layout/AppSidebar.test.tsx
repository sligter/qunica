import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'
import { TooltipProvider } from '@/components/ui/tooltip'
import { CONVERSATION_ID_MIME } from '@/lib/conversationDrag'
import { AppSidebar } from './AppSidebar'

const mocks = vi.hoisted(() => ({
  directChats: [] as Array<Record<string, unknown>>,
  groups: [] as Array<Record<string, unknown>>,
  deleteDirectChat: vi.fn(),
  renameDirectChat: vi.fn(),
  closeConversation: vi.fn(),
  closeAll: vi.fn(),
}))

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({ data: mocks.groups, isLoading: false, error: null }),
}))
vi.mock('@/hooks/useDirectChats', () => ({
  useDirectChats: () => ({ data: mocks.directChats, isLoading: false, error: null }),
  useDeleteDirectChat: () => ({ mutateAsync: mocks.deleteDirectChat, isPending: false }),
  useRenameDirectChat: () => ({
    mutateAsync: mocks.renameDirectChat,
    isPending: false,
  }),
}))
vi.mock('@/components/direct-chats/DirectChatPickerDialog', () => ({
  DirectChatPickerDialog: () => null,
}))
vi.mock('@/components/groups/GroupFormDialog', () => ({
  GroupFormDialog: () => null,
}))
vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => ({
    closeConversation: mocks.closeConversation,
    closeAll: mocks.closeAll,
  }),
}))

function LocationProbe() {
  const location = useLocation()
  return <div data-testid="location">{location.pathname}</div>
}

const initialActivity = useConversationActivityStore.getInitialState()

function renderSidebar() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/chats/chat-1']}>
        <TooltipProvider>
          <AppSidebar />
          <LocationProbe />
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('AppSidebar terminal cleanup', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    localStorage.removeItem('ag-swarmer:layout:sidebar-collapsed')
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    mocks.directChats = [{
      id: 'chat-1', title: 'Direct chat', agent_name: 'Solo',
      updated_at: '2026-07-22T00:00:00Z',
    }]
    mocks.groups = []
    mocks.deleteDirectChat.mockReset().mockResolvedValue(undefined)
    mocks.renameDirectChat.mockReset().mockResolvedValue(undefined)
    mocks.closeConversation.mockReset().mockResolvedValue(undefined)
    mocks.closeAll.mockReset().mockResolvedValue(undefined)
  })

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
  })

  async function confirmDelete() {
    const user = userEvent.setup()
    fireEvent.contextMenu(screen.getByText('Direct chat').closest('a')!)
    await user.click(screen.getByRole('menuitem', { name: 'Delete' }))
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }),
    )
  }

  it('waits for terminal cleanup after backend deletion and before navigation', async () => {
    let releaseCleanup!: () => void
    mocks.closeConversation.mockImplementationOnce(() => new Promise<void>((resolve) => {
      releaseCleanup = resolve
    }))
    renderSidebar()

    await confirmDelete()
    await waitFor(() => expect(mocks.closeConversation).toHaveBeenCalledWith('chat-1', true))
    expect(mocks.deleteDirectChat.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.closeConversation.mock.invocationCallOrder[0]!,
    )
    expect(screen.getByTestId('location')).toHaveTextContent('/chats/chat-1')

    releaseCleanup()
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/'))
  })

  it('does not clean terminal state when backend deletion fails', async () => {
    mocks.deleteDirectChat.mockRejectedValueOnce(new Error('backend refused deletion'))
    renderSidebar()

    await confirmDelete()

    expect(await screen.findByRole('alert')).toHaveTextContent('backend refused deletion')
    expect(mocks.closeConversation).not.toHaveBeenCalled()
    expect(screen.getByTestId('location')).toHaveTextContent('/chats/chat-1')
  })

  it('logs stable cleanup diagnostics and continues navigation', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    mocks.closeConversation.mockRejectedValueOnce({
      code: 'terminal.cleanup_timeout', message: 'Cleanup timed out',
    })
    renderSidebar()

    await confirmDelete()

    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/'))
    expect(consoleError).toHaveBeenCalledWith('[terminal] cleanup failed', {
      code: 'terminal.cleanup_timeout', message: 'Cleanup timed out',
    })
  })

  it('keeps conversation search collapsed until requested', async () => {
    const user = userEvent.setup()
    renderSidebar()

    expect(
      screen.queryByRole('textbox', { name: 'Search conversations' }),
    ).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Search conversations' }))
    const search = screen.getByRole('textbox', { name: 'Search conversations' })
    expect(search).toHaveFocus()

    await user.type(search, 'missing')
    expect(screen.queryByText('Direct chat')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Close' }))
    expect(
      screen.queryByRole('textbox', { name: 'Search conversations' }),
    ).not.toBeInTheDocument()
    expect(screen.getByText('Direct chat')).toBeInTheDocument()
  })

  it('renames a direct chat from its context menu', async () => {
    const user = userEvent.setup()
    renderSidebar()

    fireEvent.contextMenu(screen.getByText('Direct chat').closest('a')!)
    await user.click(screen.getByRole('menuitem', { name: 'Rename conversation' }))
    const input = screen.getByRole('textbox', { name: 'Rename conversation' })
    await user.clear(input)
    await user.type(input, 'Renamed chat')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(mocks.renameDirectChat).toHaveBeenCalledWith({ title: 'Renamed chat' }))
  })

  it('collapses both sections and mounts long lists in batches', async () => {
    const user = userEvent.setup()
    mocks.directChats = Array.from({ length: 25 }, (_, index) => ({
      id: `chat-${index + 1}`,
      title: `Chat ${String(index + 1).padStart(2, '0')}`,
      agent_name: 'Solo',
      updated_at: '2026-07-22T00:00:00Z',
    }))
    mocks.groups = [{ id: 'group-1', name: 'Group one', created_at: '2026-07-22T00:00:00Z' }]
    renderSidebar()

    expect(screen.getByText('Chat 20')).toBeInTheDocument()
    expect(screen.queryByText('Chat 21')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Load more' }))
    expect(screen.getByText('Chat 25')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Chats' }))
    expect(screen.queryByText('Chat 01')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Groups' }))
    expect(screen.queryByText('Group one')).not.toBeInTheDocument()
  })

  it('shows what each conversation is doing right now', () => {
    mocks.groups = [{ id: 'group-1', name: 'Group one', created_at: '2026-07-22T00:00:00Z' }]
    useConversationActivityStore.setState(initialActivity, true)
    useConversationActivityStore.getState().startRun({
      id: 'run-1',
      conversationId: 'group-1',
      threadId: 'thread-1',
      scope: 'groups',
    })
    renderSidebar()

    const groupRow = screen.getByText('Group one').closest('a')!
    expect(groupRow).toHaveAccessibleName(expect.stringContaining('Replying'))
    // The direct chat is idle, so it says nothing at all.
    const chatRow = screen.getByText('Direct chat').closest('a')!
    expect(chatRow).not.toHaveAccessibleName(expect.stringContaining('Replying'))
  })

  it('publishes conversation IDs when a conversation is dragged', () => {
    mocks.groups = [{ id: 'group-1', name: 'Group one', created_at: '2026-07-22T00:00:00Z' }]
    renderSidebar()
    const setData = vi.fn()
    const dataTransfer = { effectAllowed: 'none', setData }

    fireEvent.dragStart(screen.getByText('Group one').closest('a')!, { dataTransfer })

    expect(dataTransfer.effectAllowed).toBe('copy')
    expect(setData).toHaveBeenCalledWith(CONVERSATION_ID_MIME, 'group-1')
  })
})
