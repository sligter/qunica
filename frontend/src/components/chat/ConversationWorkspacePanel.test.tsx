import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, render, screen } from '@testing-library/react'
import { useEffect } from 'react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { GroupWorkspacePanel } from '@/components/chat/GroupWorkspacePanel'
import i18n from '@/i18n'
import { useFileNavStore } from '@/stores/fileNavStore'

const panelMocks = vi.hoisted(() => ({ files: vi.fn(), git: vi.fn(), computer: vi.fn(), disposed: vi.fn() }))

vi.mock('@/components/chat/ConversationComputerTab', () => ({
  ConversationComputerTab: (props: { scope: string; conversationId: string }) => {
    panelMocks.computer(props)
    useEffect(() => () => panelMocks.disposed(props.conversationId), [props.conversationId])
    return <div>computer:{props.scope}:{props.conversationId}</div>
  },
}))
vi.mock('@/components/chat/ConversationTriggersTab', () => ({
  ConversationTriggersTab: (props: { scope: string; conversationId: string }) => <div>triggers:{props.scope}:{props.conversationId}</div>,
}))

vi.mock('@/components/chat/WorkspaceFilesTab', () => ({
  WorkspaceFilesTab: (props: {
    scope: string
    conversationId: string | undefined
    workspaceId: string | null
  }) => {
    panelMocks.files(props)
    return <div>files:{props.scope}:{props.conversationId}:{props.workspaceId}</div>
  },
}))

vi.mock('@/components/chat/WorkspaceGitTab', () => ({
  WorkspaceGitTab: (props: {
    groupId: string | undefined
    scope: string
    threadId?: string
    taskBranch?: string | null
  }) => {
    panelMocks.git(props)
    return <div>git content</div>
  },
}))

function renderWithClient(element: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}>{element}</QueryClientProvider>)
}

describe('conversation workspace panels', () => {
  beforeEach(async () => {
    sessionStorage.clear()
    panelMocks.files.mockReset()
    panelMocks.git.mockReset()
    panelMocks.computer.mockReset()
    panelMocks.disposed.mockReset()
    useFileNavStore.getState().clear()
    await i18n.changeLanguage('en-US')
  })

  afterEach(cleanup)

  it('renders Files and Git for direct chats with direct file scope', async () => {
    const user = userEvent.setup()
    renderWithClient(
      <GroupWorkspacePanel
        scope="direct-chats"
        groupId="chat-1"
        workspaceId="workspace-1"
        width={320}
      />,
    )

    expect(screen.getByRole('tab', { name: 'Files' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Git' })).toBeVisible()
    expect(screen.getByText('files:direct-chats:chat-1:workspace-1')).toBeVisible()
    expect(panelMocks.files).toHaveBeenCalledWith(expect.objectContaining({
      scope: 'direct-chats',
      conversationId: 'chat-1',
      workspaceId: 'workspace-1',
    }))
    await user.click(screen.getByRole('tab', { name: 'Git' }))
    expect(panelMocks.git).toHaveBeenCalledWith(expect.objectContaining({
      groupId: 'chat-1',
      scope: 'direct-chats',
    }))
  })

  it('keeps Files and Git tabs for groups while reusing the shared Files panel', async () => {
    const user = userEvent.setup()
    renderWithClient(
      <GroupWorkspacePanel
        groupId="group-1"
        threadId="thread-1"
        taskBranch="ui-enhanced"
        workspaceId="workspace-group"
        width={320}
      />,
    )

    expect(screen.getByRole('tab', { name: 'Files' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Git' })).toBeVisible()
    expect(screen.getByText('files:groups:group-1:workspace-group')).toBeVisible()
    expect(panelMocks.files).toHaveBeenCalledWith(expect.objectContaining({
      scope: 'groups',
      conversationId: 'group-1',
      workspaceId: 'workspace-group',
    }))
    await user.click(screen.getByRole('tab', { name: 'Git' }))
    expect(panelMocks.git).toHaveBeenCalledWith(expect.objectContaining({
      groupId: 'group-1',
      scope: 'groups',
      threadId: 'thread-1',
      taskBranch: 'ui-enhanced',
    }))
  })

  it('loads the added panels only when selected and keeps file navigation working', async () => {
    const user = userEvent.setup()
    renderWithClient(<GroupWorkspacePanel groupId="group-1" />)
    expect(panelMocks.computer).not.toHaveBeenCalled()
    await user.click(screen.getByRole('tab', { name: 'Computer' }))
    expect(await screen.findByText('computer:groups:group-1')).toBeVisible()
    await user.click(screen.getByRole('tab', { name: 'Triggers' }))
    expect(await screen.findByText('triggers:groups:group-1')).toBeVisible()
    expect(panelMocks.disposed).toHaveBeenCalledWith('group-1')
    expect(sessionStorage.getItem('qunica:groups:workspace-panel-tab')).toBe('triggers')
    act(() => useFileNavStore.getState().openFile('group-1', 'README.md'))
    expect(screen.getByRole('tab', { name: 'Files' })).toHaveAttribute('data-state', 'active')
  })

  it('restores the Computer tab and replaces the view when conversations change', async () => {
    sessionStorage.setItem('qunica:groups:workspace-panel-tab', 'computer')
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const frame = (id: string) => <QueryClientProvider client={client}><GroupWorkspacePanel groupId={id} /></QueryClientProvider>
    const view = render(frame('group-1'))
    expect(await screen.findByText('computer:groups:group-1')).toBeVisible()
    view.rerender(frame('group-2'))
    expect(await screen.findByText('computer:groups:group-2')).toBeVisible()
    expect(screen.queryByText('computer:groups:group-1')).not.toBeInTheDocument()
    expect(panelMocks.disposed).toHaveBeenCalledWith('group-1')
  })
})
