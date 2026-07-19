import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConversationChatView } from './ConversationChatView'

vi.mock('@/components/chat/Composer', () => ({
  Composer: ({ allowMentions, disabledReason }: { allowMentions?: boolean; disabledReason?: string }) => (
    <div>
      composer:{String(allowMentions)}:{disabledReason ?? 'enabled'}
    </div>
  ),
}))
vi.mock('@/components/chat/GroupWorkspacePanel', () => ({
  GroupWorkspacePanel: () => <div>workspace panel</div>,
}))
vi.mock('@/components/chat/MessageList', () => ({ MessageList: () => <div>message list</div> }))
vi.mock('@/components/chat/TurnTraceDrawer', () => ({ TurnTraceDrawer: () => <div>turn trace</div> }))
vi.mock('@/components/layout/VerticalResizeHandle', () => ({ VerticalResizeHandle: () => <div /> }))
vi.mock('@/hooks/useGroupMessages', () => ({
  useConversationMessages: () => ({
    error: null,
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  }),
}))
vi.mock('@/hooks/usePersistentPaneWidth', () => ({
  usePersistentPaneWidth: () => ({
    width: 280,
    minWidth: 240,
    maxWidth: 560,
    startResize: vi.fn(),
    resizeBy: vi.fn(),
  }),
}))
vi.mock('@/hooks/useSendMessageStream', () => ({
  useSendMessageStream: () => ({ error: null, isStreaming: false, send: vi.fn(), cancel: vi.fn() }),
}))
vi.mock('@/stores/fileNavStore', () => ({ useFileNavStore: () => null }))
vi.mock('@/stores/messageStore', () => ({ useMessageStore: () => vi.fn() }))

describe('ConversationChatView', () => {
  it('keeps direct chat canvas capabilities while omitting group-only controls', () => {
    render(
      <ConversationChatView
        conversationId="chat-1"
        scope="direct-chats"
        schedulerEnabled={false}
        agents={[]}
        title="Direct chat"
        subtitle="Solo"
        announcement="group only"
        headerActions={<button>Manage Group</button>}
        capabilities={{
          showAnnouncement: false,
          showManage: false,
          showTurnTrace: false,
          showWorkspace: true,
          allowMentions: false,
        }}
      />,
    )

    expect(screen.getByText('message list')).toBeInTheDocument()
    expect(screen.getByText('composer:false:enabled')).toBeInTheDocument()
    expect(screen.getByText('workspace panel')).toBeInTheDocument()
    expect(screen.queryByText('Manage Group')).not.toBeInTheDocument()
    expect(screen.queryByText('group only')).not.toBeInTheDocument()
    expect(screen.queryByText('turn trace')).not.toBeInTheDocument()
  })
})
