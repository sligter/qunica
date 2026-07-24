import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { TooltipProvider } from '@/components/ui/tooltip'
import { AppSidebar } from './AppSidebar'

const mocks = vi.hoisted(() => ({
  directChats: [] as Array<Record<string, unknown>>,
  deleteDirectChat: vi.fn(),
  closeConversation: vi.fn(),
  closeAll: vi.fn(),
}))

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({ data: [], isLoading: false, error: null }),
}))
vi.mock('@/hooks/useDirectChats', () => ({
  useDirectChats: () => ({ data: mocks.directChats, isLoading: false, error: null }),
  useDeleteDirectChat: () => ({ mutateAsync: mocks.deleteDirectChat, isPending: false }),
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
    mocks.deleteDirectChat.mockReset().mockResolvedValue(undefined)
    mocks.closeConversation.mockReset().mockResolvedValue(undefined)
    mocks.closeAll.mockReset().mockResolvedValue(undefined)
  })

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
  })

  async function confirmDelete() {
    const user = userEvent.setup()
    await user.click(screen.getByRole('button', { name: 'Delete conversation Direct chat' }))
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
})
