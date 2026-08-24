import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MessageList } from '@/components/chat/MessageList'
import i18n from '@/i18n'
import { useMessageStore, type StreamRun } from '@/stores/messageStore'
import type { Message } from '@/types/api'

Element.prototype.scrollIntoView = () => {}

vi.mock('@/components/chat/MessageItem', () => ({
  MessageItem: ({ message, threadId }: { message: Message; threadId?: string }) => (
    <div data-testid={`message-${message.id}`} data-thread-id={threadId ?? ''}>
      {message.content}
    </div>
  ),
}))

vi.mock('@/components/chat/StreamTimeline', () => ({
  StreamTimeline: () => <div data-testid="stream-timeline" />,
}))

const userMessage: Message = {
  id: 'message-1',
  group_id: 'group-1',
  thread_id: 'thread-1',
  sender_type: 'user',
  sender_id: 'user-1',
  message_type: 'text',
  content: 'Run the group',
  attachments: [],
  status: 'completed',
  refs: null,
  context_usage: null,
  turn_id: 'turn-persisted',
  dispatch_id: null,
  reply_to_message_id: null,
  turn_summary: { status: 'completed', termination_reason: null },
  created_at: '2026-07-15T10:00:00Z',
}

function setMessageState(run?: StreamRun, messages: Message[] = [userMessage]) {
  useMessageStore.setState({
    byGroup: { 'group-1': messages },
    warningsByGroup: {},
    streamRunsByGroup: run ? { 'group-1': { [run.id]: run } } : {},
    streamRunIdByUserMessageIdByGroup: run
      ? { 'group-1': { [userMessage.id]: run.id } }
      : {},
  })
}

afterEach(async () => {
  cleanup()
  sessionStorage.clear()
  useMessageStore.setState({
    byGroup: {},
    warningsByGroup: {},
    streamRunsByGroup: {},
    streamRunIdByUserMessageIdByGroup: {},
  })
  await i18n.changeLanguage('en-US')
})

describe('MessageList scheduler summary integration', () => {
  it('gives the system Assistant useful first prompts instead of an @mention hint', async () => {
    const user = userEvent.setup()
    const onSubmitHumanInput = vi.fn()
    await i18n.changeLanguage('zh-CN')

    render(
      <MessageList
        groupId="assistant-chat"
        agentIsSystem
        onSubmitHumanInput={onSubmitHumanInput}
      />,
    )

    expect(screen.getByText('今天想先处理什么？')).toBeVisible()
    expect(screen.queryByText(/@AgentName/)).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '检查当前配置，告诉我还缺什么' }))
    expect(onSubmitHumanInput).toHaveBeenCalledWith('检查当前配置，告诉我还缺什么')
  })

  it('hands rows the thread its message query is keyed by', () => {
    setMessageState()
    render(<MessageList groupId="group-1" threadId="thread-1" />)
    expect(screen.getByTestId('message-message-1')).toHaveAttribute('data-thread-id', 'thread-1')

    // A direct chat reads its messages without a thread, even though each
    // message carries one; rows must not invent one from the message.
    cleanup()
    render(<MessageList groupId="chat-1" scope="direct-chats" stateId="group-1" />)
    expect(screen.getByTestId('message-message-1')).toHaveAttribute('data-thread-id', '')
  })

  it('localizes older-message loading while preserving Agent-authored content', async () => {
    setMessageState()
    await i18n.changeLanguage('en-US')
    render(<MessageList groupId="group-1" hasOlderMessages isLoadingOlderMessages />)
    expect(screen.getByRole('button', { name: 'Loading older messages…' })).toBeDisabled()
    expect(screen.getByText('Run the group')).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    render(<MessageList groupId="group-1" hasOlderMessages isLoadingOlderMessages />)
    expect(screen.getByRole('button', { name: '正在加载更早的消息…' })).toBeDisabled()
    expect(screen.getByText('Run the group')).toBeVisible()
  })

  it('localizes known warnings and preserves unknown warning detail', async () => {
    setMessageState()
    useMessageStore.setState({ warningsByGroup: { 'group-1': ['No one replied'] } })
    render(<MessageList groupId="group-1" />)
    expect(screen.getByText('No one replied.')).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    useMessageStore.setState({ warningsByGroup: { 'group-1': ['No one replied'] } })
    render(<MessageList groupId="group-1" />)
    expect(screen.getByText('无人回复。')).toBeVisible()

    cleanup()
    useMessageStore.setState({ warningsByGroup: { 'group-1': ['RAW_WARNING_DETAIL'] } })
    render(<MessageList groupId="group-1" />)
    expect(screen.getByText('RAW_WARNING_DETAIL')).toBeVisible()
  })

  it('localizes known scheduler summaries and preserves unknown summary detail', async () => {
    const run: StreamRun = {
      id: 'stream-summary',
      group_id: 'group-1',
      user_message_id: userMessage.id,
      status: 'active',
      turn_id: 'turn-summary',
      scheduler_status: 'running',
      moderator_active: false,
      terminal_reason: null,
      criticalSummaries: [
        {
          id: 'call-1',
          kind: 'call',
          message: 'Agent call routed to agent-2',
          count: 1,
          target_agent_id: 'agent-2',
          created_at: '2026-07-15T10:00:01Z',
        },
        {
          id: 'custom-1',
          kind: 'handoff',
          message: 'RAW_SUMMARY_DETAIL',
          count: 1,
          created_at: '2026-07-15T10:00:02Z',
        },
      ],
      created_at: '2026-07-15T10:00:00Z',
      updated_at: '2026-07-15T10:00:02Z',
      events: [],
    }
    setMessageState(run)
    render(<MessageList groupId="group-1" onViewTurnTrace={vi.fn()} />)
    expect(screen.getByText('Agent call routed to agent-2')).toBeVisible()
    expect(screen.getByText('RAW_SUMMARY_DETAIL')).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    render(<MessageList groupId="group-1" onViewTurnTrace={vi.fn()} />)
    expect(screen.getByText('Agent 调用已路由至 agent-2')).toBeVisible()
    expect(screen.getByText('RAW_SUMMARY_DETAIL')).toBeVisible()
  })
  it('keeps the message column shrinkable and clips page-level horizontal overflow', () => {
    setMessageState()

    const { container } = render(<MessageList groupId="group-1" />)

    const scrollRoot = container.firstElementChild
    const messageColumn = scrollRoot?.firstElementChild
    expect(scrollRoot).toHaveClass('min-w-0', 'overflow-x-hidden', 'overflow-y-auto')
    expect(messageColumn).toHaveClass('min-w-0', 'w-full', 'max-w-6xl')
  })

  it('shows a jump-to-latest button whenever the user scrolls away from the bottom', async () => {
    const user = userEvent.setup()
    const scrollIntoView = vi.spyOn(Element.prototype, 'scrollIntoView')
    setMessageState()

    const { container } = render(<MessageList groupId="group-1" />)
    const scrollRoot = container.firstElementChild as HTMLDivElement
    Object.defineProperties(scrollRoot, {
      scrollHeight: { configurable: true, value: 1000 },
      clientHeight: { configurable: true, value: 500 },
      scrollTop: { configurable: true, writable: true, value: 0 },
    })

    fireEvent.scroll(scrollRoot)
    const button = screen.getByRole('button', { name: 'Jump to latest' })
    await user.click(button)

    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'smooth', block: 'end' })
    expect(button).not.toBeInTheDocument()
    scrollIntoView.mockRestore()
  })

  it('opens a conversation at the latest message when no scroll position is saved', async () => {
    const { container } = render(<MessageList groupId="group-1" />)
    const scrollRoot = container.firstElementChild as HTMLDivElement
    Object.defineProperties(scrollRoot, {
      scrollHeight: { configurable: true, value: 1000 },
      clientHeight: { configurable: true, value: 500 },
      scrollTop: { configurable: true, writable: true, value: 0 },
    })

    act(() => setMessageState())

    await waitFor(() => expect(scrollRoot.scrollTop).toBe(500))
  })

  it('anchors a persisted turn summary below its trigger message', async () => {
    const user = userEvent.setup()
    const onViewTurnTrace = vi.fn()
    setMessageState()

    render(<MessageList groupId="group-1" onViewTurnTrace={onViewTurnTrace} />)

    const message = screen.getByTestId('message-message-1')
    const summary = screen.getByRole('region', { name: 'Scheduler turn summary' })
    expect(message.nextElementSibling).toBe(summary)
    expect(summary).toHaveTextContent('Completed')

    await user.click(screen.getByRole('button', { name: 'View trace' }))
    expect(onViewTurnTrace).toHaveBeenCalledWith(
      'turn-persisted',
      screen.getByRole('button', { name: 'View trace' }),
    )
  })

  it('prefers the live turn status and critical summaries while streaming', () => {
    setMessageState({
      id: 'stream-1',
      group_id: 'group-1',
      user_message_id: userMessage.id,
      status: 'active',
      turn_id: 'turn-live',
      scheduler_status: 'running',
      moderator_active: false,
      terminal_reason: null,
      criticalSummaries: [
        {
          id: 'summary-1',
          kind: 'handoff',
          message: 'Agent handoff scheduled',
          count: 1,
          created_at: '2026-07-15T10:00:01Z',
        },
      ],
      created_at: '2026-07-15T10:00:00Z',
      updated_at: '2026-07-15T10:00:01Z',
      events: [],
    })

    render(<MessageList groupId="group-1" onViewTurnTrace={vi.fn()} />)

    const summary = screen.getByRole('region', { name: 'Scheduler turn summary' })
    expect(summary).toHaveTextContent('Running')
    expect(summary).toHaveTextContent('Agent handoff scheduled')
  })

  it('renders an interrupted checkpoint instead of its duplicate unfinished draft', () => {
    const run: StreamRun = {
      id: 'stream-interrupted',
      group_id: 'group-1',
      user_message_id: userMessage.id,
      status: 'error',
      turn_id: 'turn-interrupted',
      scheduler_status: 'failed',
      moderator_active: false,
      terminal_reason: 'persistence_failed',
      criticalSummaries: [],
      created_at: '2026-07-15T10:00:00Z',
      updated_at: '2026-07-15T10:00:02Z',
      events: [
        {
          id: 'completed-1',
          type: 'agent_message',
          stream_id: 'stream-interrupted',
          message_id: 'completed-1',
          agent_id: 'agent-2',
          display_name: 'Agent Two',
          content: 'Earlier reply',
          created_at: '2026-07-15T10:00:00Z',
        },
        {
          id: 'draft-1',
          type: 'response_draft',
          stream_id: 'stream-interrupted',
          agent_id: 'agent-1',
          display_name: 'Agent One',
          content: 'Partial reply',
          status: 'streaming',
          created_at: '2026-07-15T10:00:01Z',
        },
      ],
    }
    const checkpoint: Message = {
      ...userMessage,
      id: 'checkpoint-1',
      sender_type: 'agent',
      sender_id: 'agent-1',
      content: 'Partial reply continued',
      status: 'interrupted',
      reply_to_message_id: null,
      turn_summary: null,
    }
    const completed: Message = {
      ...checkpoint,
      id: 'completed-1',
      sender_id: 'agent-2',
      content: 'Earlier reply',
      status: 'visible',
      reply_to_message_id: userMessage.id,
    }
    setMessageState(run, [userMessage, completed, checkpoint])

    render(<MessageList groupId="group-1" />)

    expect(screen.getByTestId('message-checkpoint-1')).toHaveTextContent(
      'Partial reply continued',
    )
    expect(screen.getByTestId('message-completed-1')).toHaveTextContent('Earlier reply')
    expect(screen.queryByTestId('stream-timeline')).not.toBeInTheDocument()
  })
})
