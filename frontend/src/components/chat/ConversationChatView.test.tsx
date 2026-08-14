import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { useState } from 'react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ConversationChatView } from './ConversationChatView'
import i18n from '@/i18n'

const terminalMocks = vi.hoisted(() => ({
  register: vi.fn(),
  toggleDock: vi.fn(),
  isDockOpen: false,
}))

const composerMocks = vi.hoisted(() => ({ render: vi.fn() }))
const messageListMocks = vi.hoisted(() => ({ render: vi.fn() }))
const workspacePanelMocks = vi.hoisted(() => ({ group: vi.fn() }))
const maintenanceMocks = vi.hoisted(() => ({
  clear: vi.fn(),
  reset: vi.fn(),
}))
const messageStoreMocks = vi.hoisted(() => ({
  clearGroupMessages: vi.fn(),
  clearWarnings: vi.fn(),
  activeResumesByMessageId: {} as Record<
    string,
    { state_id: string; cancel: () => Promise<void> }
  >,
}))

vi.mock('@/components/chat/Composer', () => ({
  Composer: (props: {
    allowMentions?: boolean
    conversationId?: string
    disabledReason?: string
    isStreaming?: boolean
    onCancel?: () => void
    scope?: 'groups' | 'direct-chats'
    workspaceId?: string | null
  }) => {
    composerMocks.render(props)
    const [mountedConversationId] = useState(props.conversationId)
    return (
      <div>
        composer:{String(props.allowMentions)}:{props.disabledReason ?? 'enabled'}
        <span data-testid="composer-instance">{mountedConversationId}</span>
        <input aria-label="Message" />
        <textarea aria-label="Message draft" />
        <select aria-label="Message mode"><option>default</option></select>
        <div aria-label="Rich message" contentEditable />
      </div>
    )
  },
}))
vi.mock('@/components/chat/GroupWorkspacePanel', () => ({
  GroupWorkspacePanel: (props: {
    groupId: string
    scope?: 'groups' | 'direct-chats'
    workspaceId: string | null
  }) => {
    workspacePanelMocks.group(props)
    return <div>workspace panel</div>
  },
}))
vi.mock('@/components/chat/MessageList', () => ({
  MessageList: (props: { stateId?: string }) => {
    messageListMocks.render(props)
    return <div>message list</div>
  },
}))
vi.mock('@/components/chat/WorkspaceEditorStage', () => ({
  WorkspaceEditorStage: ({ children }: { children: React.ReactNode }) => children,
}))
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
  useClearConversationMessages: () => ({
    mutateAsync: maintenanceMocks.clear,
    isPending: false,
  }),
  useResetDirectChatContext: () => ({
    mutateAsync: maintenanceMocks.reset,
    isPending: false,
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
vi.mock('@/stores/messageStore', () => ({
  useMessageStore: (selector: (state: typeof messageStoreMocks) => unknown) => (
    selector(messageStoreMocks)
  ),
}))
vi.mock('@/terminal/useTerminalConversationRegistration', () => ({
  useTerminalConversationRegistration: terminalMocks.register,
}))
vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => ({
    isDockOpen: terminalMocks.isDockOpen,
    toggleDock: terminalMocks.toggleDock,
  }),
}))

interface ConversationRenderOptions {
  conversationId?: string
  scope?: 'groups' | 'direct-chats'
  threadId?: string
  workspaceId?: string | null
}

function conversationElement({
  conversationId = 'chat-1',
  scope = 'direct-chats',
  threadId,
  workspaceId = 'workspace-1',
}: ConversationRenderOptions = {}) {
  return (
    <I18nextProvider i18n={i18n}>
      <ConversationChatView
        conversationId={conversationId}
        threadId={threadId}
        workspaceId={workspaceId}
        scope={scope}
        agents={[]}
        title="Direct chat"
        subtitle="Solo"
        announcement="group only"
        renderHeaderContext={scope === 'groups' ? () => <button>Start new task</button> : undefined}
        headerActions={<button>Manage Group</button>}
        capabilities={{
          showAnnouncement: false,
          showManage: false,
          showTurnTrace: false,
          showWorkspace: true,
          allowMentions: false,
        }}
      />
    </I18nextProvider>
  )
}

function renderConversation(options: ConversationRenderOptions = {}) {
  return render(conversationElement(options))
}

describe('ConversationChatView', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    terminalMocks.register.mockReset()
    terminalMocks.toggleDock.mockReset().mockResolvedValue(undefined)
    terminalMocks.isDockOpen = false
    composerMocks.render.mockReset()
    messageListMocks.render.mockReset()
    workspacePanelMocks.group.mockReset()
    maintenanceMocks.clear.mockReset().mockResolvedValue(undefined)
    maintenanceMocks.reset.mockReset().mockResolvedValue(undefined)
    messageStoreMocks.clearGroupMessages.mockReset()
    messageStoreMocks.clearWarnings.mockReset()
    messageStoreMocks.activeResumesByMessageId = {}
    Object.defineProperty(navigator, 'platform', { configurable: true, value: 'Win32' })
  })

  afterEach(cleanup)

  it('keeps direct chat canvas capabilities while omitting group-only controls', () => {
    renderConversation()

    expect(screen.getByText('message list')).toBeInTheDocument()
    expect(screen.getByText('composer:false:enabled')).toBeInTheDocument()
    expect(screen.getByText('workspace panel')).toBeInTheDocument()
    expect(screen.queryByText('Manage Group')).not.toBeInTheDocument()
    expect(screen.queryByText('group only')).not.toBeInTheDocument()
    expect(screen.queryByText('turn trace')).not.toBeInTheDocument()
    expect(composerMocks.render).toHaveBeenCalledWith(expect.objectContaining({
      conversationId: 'chat-1',
      workspaceId: 'workspace-1',
      scope: 'direct-chats',
    }))
    expect(workspacePanelMocks.group).toHaveBeenCalledWith(expect.objectContaining({
      groupId: 'chat-1',
      workspaceId: 'workspace-1',
      scope: 'direct-chats',
    }))
  })

  it('places private-chat maintenance actions before the terminal controls', () => {
    const view = renderConversation()

    const reset = screen.getByRole('button', { name: 'Reset context' })
    const clear = screen.getByRole('button', { name: 'Clear chat' })
    const terminal = screen.getByRole('button', { name: 'Show terminal' })
    expect(reset).not.toHaveTextContent('Reset context')
    expect(clear).not.toHaveTextContent('Clear chat')
    expect(reset.compareDocumentPosition(terminal) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(clear.compareDocumentPosition(terminal) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()

    view.rerender(conversationElement({ scope: 'groups' }))
    expect(screen.queryByRole('button', { name: 'Reset context' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Clear chat' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Start new task' })).toBeVisible()
  })

  it('switches task-local message state without clearing the background task', () => {
    const view = renderConversation({ scope: 'groups', threadId: 'thread-1' })
    expect(messageListMocks.render).toHaveBeenLastCalledWith(
      expect.objectContaining({ stateId: 'thread-1' }),
    )

    view.rerender(conversationElement({ scope: 'groups', threadId: 'thread-2' }))

    expect(messageListMocks.render).toHaveBeenLastCalledWith(
      expect.objectContaining({ stateId: 'thread-2' }),
    )
    expect(messageStoreMocks.clearGroupMessages).not.toHaveBeenCalled()
  })

  it('confirms private-chat context reset and message clearing', async () => {
    const user = userEvent.setup()
    renderConversation()

    await user.click(screen.getByRole('button', { name: 'Reset context' }))
    let dialog = screen.getByRole('alertdialog')
    await user.click(within(dialog).getByRole('button', { name: 'Reset context' }))
    expect(maintenanceMocks.reset).toHaveBeenCalledTimes(1)

    await user.click(screen.getByRole('button', { name: 'Clear chat' }))
    dialog = screen.getByRole('alertdialog')
    await user.click(within(dialog).getByRole('button', { name: 'Clear chat' }))
    expect(maintenanceMocks.clear).toHaveBeenCalledTimes(1)
  })

  it('registers its workspace and toggles the terminal from the header', async () => {
    const user = userEvent.setup()
    renderConversation()

    expect(terminalMocks.register).toHaveBeenCalledWith('chat-1', 'workspace-1')
    const button = screen.getByRole('button', { name: 'Show terminal' })
    expect(button).toHaveAttribute('aria-pressed', 'false')
    await user.click(button)
    expect(terminalMocks.toggleDock).toHaveBeenCalledTimes(1)
  })

  it('passes group conversation file scope and workspace identity to the composer', () => {
    renderConversation({
      conversationId: 'group-1',
      scope: 'groups',
      workspaceId: 'workspace-group',
    })

    expect(composerMocks.render).toHaveBeenCalledWith(expect.objectContaining({
      conversationId: 'group-1',
      workspaceId: 'workspace-group',
      scope: 'groups',
    }))
    expect(screen.getByText('workspace panel')).toBeInTheDocument()
    expect(workspacePanelMocks.group).toHaveBeenCalledWith(expect.objectContaining({
      groupId: 'group-1',
      scope: 'groups',
      workspaceId: 'workspace-group',
    }))
  })

  it('remounts the composer when the conversation identity changes', () => {
    const view = renderConversation({ conversationId: 'chat-1', workspaceId: 'workspace-1' })
    expect(screen.getByTestId('composer-instance')).toHaveTextContent('chat-1')

    view.rerender(conversationElement({
      conversationId: 'chat-2',
      workspaceId: 'workspace-2',
    }))

    expect(screen.getByTestId('composer-instance')).toHaveTextContent('chat-2')
  })

  it('shows streaming controls and stops an active resumed message', () => {
    const cancelResume = vi.fn().mockResolvedValue(undefined)
    messageStoreMocks.activeResumesByMessageId = {
      'message-1': { state_id: 'chat-1', cancel: cancelResume },
    }
    renderConversation()

    const props = composerMocks.render.mock.lastCall?.[0]
    expect(props).toEqual(expect.objectContaining({ isStreaming: true }))
    props?.onCancel?.()
    expect(cancelResume).toHaveBeenCalledTimes(1)
  })

  it('handles Ctrl+` on Windows and ignores unsafe or ordinary key events', () => {
    renderConversation()

    const handled = new KeyboardEvent('keydown', {
      key: '`', ctrlKey: true, bubbles: true, cancelable: true,
    })
    window.dispatchEvent(handled)
    expect(handled.defaultPrevented).toBe(true)
    expect(terminalMocks.toggleDock).toHaveBeenCalledTimes(1)

    fireEvent.keyDown(window, { key: '`', ctrlKey: true, repeat: true })
    fireEvent.keyDown(window, { key: '`', metaKey: true })
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Message' }), { key: '`' })
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Message' }), {
      key: '`', ctrlKey: true, isComposing: true,
    })
    const prevented = new KeyboardEvent('keydown', {
      key: '`', ctrlKey: true, bubbles: true, cancelable: true,
    })
    prevented.preventDefault()
    window.dispatchEvent(prevented)

    expect(terminalMocks.toggleDock).toHaveBeenCalledTimes(1)
  })

  it('does not capture Ctrl+` from editable controls on Windows', () => {
    renderConversation()

    for (const target of [
      screen.getByRole('textbox', { name: 'Message' }),
      screen.getByRole('textbox', { name: 'Message draft' }),
      screen.getByRole('combobox', { name: 'Message mode' }),
    ]) {
      const event = new KeyboardEvent('keydown', {
        key: '`', ctrlKey: true, bubbles: true, cancelable: true,
      })
      target.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(false)
    }

    expect(terminalMocks.toggleDock).not.toHaveBeenCalled()
  })

  it('uses Meta+` rather than Ctrl+` on macOS', () => {
    Object.defineProperty(navigator, 'platform', { configurable: true, value: 'MacIntel' })
    renderConversation()

    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(terminalMocks.toggleDock).not.toHaveBeenCalled()
    fireEvent.keyDown(window, { key: '`', metaKey: true })
    expect(terminalMocks.toggleDock).toHaveBeenCalledTimes(1)
  })

  it('does not capture Meta+` from a contenteditable target on macOS', () => {
    Object.defineProperty(navigator, 'platform', { configurable: true, value: 'MacIntel' })
    renderConversation()

    const event = new KeyboardEvent('keydown', {
      key: '`', metaKey: true, bubbles: true, cancelable: true,
    })
    screen.getByLabelText('Rich message').dispatchEvent(event)

    expect(event.defaultPrevented).toBe(false)
    expect(terminalMocks.toggleDock).not.toHaveBeenCalled()
  })
})
