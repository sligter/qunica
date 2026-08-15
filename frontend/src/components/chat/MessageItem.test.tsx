import { act, cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MessageItem } from '@/components/chat/MessageItem'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

vi.mock('@/hooks/useGroupAgents', () => ({
  useGroupAgents: () => ({
    data: [
      {
        agent_id: 'agent-1',
        display_name: 'Researcher',
        context_usage: null,
      },
    ],
  }),
}))

vi.mock('@/components/chat/MarkdownMessage', () => ({
  MarkdownMessage: ({ content }: { content: string }) => <div>{content}</div>,
}))

vi.mock('@/components/chat/MessageActions', () => ({
  MessageActions: () => null,
}))

vi.mock('@/components/chat/MessageAttachments', () => ({
  MessageAttachments: ({ attachments }: { attachments: Array<{ name: string }> }) => (
    <div>{attachments.map((attachment) => attachment.name).join(', ')}</div>
  ),
}))

vi.mock('@/components/chat/InterruptedMessageActions', () => ({
  InterruptedMessageActions: () => <div data-testid="interrupted-message-actions" />,
}))

function message(overrides: Partial<Message>): Message {
  return {
    id: 'message-1',
    group_id: 'group-1',
    thread_id: 'thread-1',
    sender_type: 'agent',
    sender_id: 'agent-1',
    message_type: 'text',
    content: 'Final answer',
    attachments: [],
    status: 'visible',
    refs: null,
    context_usage: null,
    response_segments: null,
    reasoning: null,
    tool_calls: null,
    turn_id: null,
    dispatch_id: null,
    reply_to_message_id: null,
    turn_summary: null,
    created_at: '2026-07-16T10:00:00Z',
    ...overrides,
  }
}

afterEach(() => {
  cleanup()
  useAuthStore.setState({ user: null })
  useMessageStore.setState({
    resumingMessageIds: new Set(),
    pendingMessageIds: new Set(),
    activeResumesByMessageId: {},
  })
})

describe('MessageItem', () => {
  it('publishes its source text for the copy menu', () => {
    render(<MessageItem groupId="group-1" message={message({ content: '# Heading\n\nBody' })} />)

    // The renderer lays out headings and links; a copy has to yield what the
    // agent actually wrote, so the row carries the source itself.
    expect(document.getElementById('message-message-1')).toHaveAttribute(
      'data-copy-text',
      '# Heading\n\nBody',
    )
  })

  it('shows a locally echoed message as sending until the server acknowledges it', () => {
    useMessageStore.setState({ pendingMessageIds: new Set(['message-1']) })
    render(<MessageItem groupId="group-1" message={message({ sender_type: 'user' })} />)

    expect(screen.getByText('Sending…')).toBeVisible()
    // A time it does not have yet would be a lie, so the row shows none.
    expect(screen.queryByText('10:00')).not.toBeInTheDocument()
  })

  it('renders persisted reasoning and tools through one collapsed activity bubble', async () => {
    const user = userEvent.setup()
    render(
      <MessageItem
        groupId="group-1"
        message={message({
          reasoning: ['First thought', 'Second thought'],
          tool_calls: [
            {
              tool_call_id: 'tool-1',
              tool_name: 'Read',
              status: 'completed',
              args_summary: 'README.md',
              result_summary: 'Project details',
            },
            {
              tool_call_id: 'tool-2',
              tool_name: 'Grep',
              status: 'completed',
              args_summary: 'Activity',
              result_summary: '4 matches',
            },
          ],
        })}
      />,
    )

    const activity = screen.getByRole('group', {
      name: 'Activity: 2 reasoning, 2 tools',
    }) as HTMLDetailsElement
    expect(activity.open).toBe(false)
    expect(screen.getAllByRole('group', { name: /activity:/i })).toHaveLength(1)

    await user.click(activity.querySelector(':scope > summary') as HTMLElement)
    expect(within(activity).getByText('First thought')).toBeVisible()
    expect(within(activity).getByText('Second thought')).toBeVisible()
    expect(within(activity).getByText('Read')).toBeVisible()
    expect(within(activity).getByText('Grep')).toBeVisible()
    expect(screen.getByText('Final answer')).toBeVisible()
  })

  it('restores persisted response segments as separate bubbles', () => {
    render(
      <MessageItem
        groupId="group-1"
        message={message({
          content: 'First bubbleSecond bubble',
          response_segments: ['First bubble', 'Second bubble'],
        })}
      />,
    )

    const firstBubble = screen.getByText('First bubble').closest('.rounded-lg')
    const secondBubble = screen.getByText('Second bubble').closest('.rounded-lg')
    expect(firstBubble).not.toBeNull()
    expect(secondBubble).not.toBeNull()
    expect(firstBubble).not.toBe(secondBubble)
  })

  it('keeps long user content inside a shrinkable right-aligned column', () => {
    useAuthStore.setState({
      user: {
        id: 'user-1',
        email: 'user@example.com',
        name: 'User',
        avatar_url: null,
        created_at: '2026-07-16T10:00:00Z',
      },
    })
    const longContent = 'unbroken-content-'.repeat(40)
    const { container } = render(
      <MessageItem
        groupId="group-1"
        message={message({
          id: 'user-message',
          sender_type: 'user',
          sender_id: 'user-1',
          content: longContent,
        })}
      />,
    )

    const row = container.querySelector('#message-user-message')
    const contentColumn = row?.querySelector(':scope > div:last-child')
    const bubble = screen.getByText(longContent).closest('.chat-user-bubble')
    expect(row).toHaveClass('min-w-0', 'w-full')
    expect(contentColumn).toHaveClass('min-w-0', 'flex-1', 'ml-auto', 'max-w-[72%]', 'items-end')
    expect(bubble).toHaveClass('min-w-0', 'max-w-full')
  })

  it('renders durable attachments for a message with empty text', () => {
    render(
      <MessageItem
        groupId="group-1"
        message={message({
          content: '',
          attachments: [{ id: 'attachment-1', path: 'uploads/photo.png', name: 'photo.png', mime_type: 'image/png', size: 1280, kind: 'image' }],
        })}
      />,
    )

    expect(screen.getByText('photo.png')).toBeVisible()
  })

  it('keeps the resume stream owner mounted until the resumed message finishes', () => {
    const interrupted = message({ status: 'interrupted' })
    const { rerender } = render(
      <MessageItem groupId="group-1" message={interrupted} />,
    )

    expect(screen.getByTestId('interrupted-message-actions')).toBeInTheDocument()

    act(() => useMessageStore.getState().startResume(
      'group-1',
      interrupted.id,
      async () => undefined,
    ))
    rerender(
      <MessageItem
        groupId="group-1"
        message={{ ...interrupted, status: 'visible' }}
      />,
    )
    expect(screen.getByTestId('interrupted-message-actions')).toBeInTheDocument()

    act(() => useMessageStore.getState().endResume(interrupted.id))
    expect(screen.queryByTestId('interrupted-message-actions')).not.toBeInTheDocument()
  })
})
