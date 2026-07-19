import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MessageItem } from '@/components/chat/MessageItem'
import { useAuthStore } from '@/stores/authStore'
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
})

describe('MessageItem', () => {
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
})
