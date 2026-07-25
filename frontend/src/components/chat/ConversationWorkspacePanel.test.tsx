import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ConversationWorkspacePanel } from '@/components/chat/ConversationWorkspacePanel'
import { GroupWorkspacePanel } from '@/components/chat/GroupWorkspacePanel'
import i18n from '@/i18n'

const panelMocks = vi.hoisted(() => ({ files: vi.fn() }))

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
  WorkspaceGitTab: () => <div>git content</div>,
}))

function renderWithClient(element: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}>{element}</QueryClientProvider>)
}

describe('conversation workspace panels', () => {
  beforeEach(async () => {
    sessionStorage.clear()
    panelMocks.files.mockReset()
    await i18n.changeLanguage('en-US')
  })

  afterEach(cleanup)

  it('renders a direct-chat panel with only the Files tab and shared context', () => {
    renderWithClient(
      <ConversationWorkspacePanel
        scope="direct-chats"
        conversationId="chat-1"
        workspaceId="workspace-1"
        width={320}
      />,
    )

    expect(screen.getByRole('tab', { name: 'Files' })).toBeVisible()
    expect(screen.queryByRole('tab', { name: 'Git' })).toBeNull()
    expect(screen.getByText('files:direct-chats:chat-1:workspace-1')).toBeVisible()
    expect(panelMocks.files).toHaveBeenCalledWith(expect.objectContaining({
      scope: 'direct-chats',
      conversationId: 'chat-1',
      workspaceId: 'workspace-1',
    }))
  })

  it('keeps Files and Git tabs for groups while reusing the shared Files panel', () => {
    renderWithClient(
      <GroupWorkspacePanel
        groupId="group-1"
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
  })
})
