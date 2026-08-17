import { cleanup, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import {
  ConversationStatusIndicator,
  ThreadStatusIndicator,
} from '@/components/chat/ConversationStatusDot'
import i18n from '@/i18n'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'

const initialActivity = useConversationActivityStore.getInitialState()

function startRun(id: string, threadId: string | null) {
  useConversationActivityStore.getState().startRun({
    id,
    conversationId: 'group-1',
    threadId,
    scope: 'groups',
  })
}

describe('conversation status indicators', () => {
  beforeEach(async () => {
    cleanup()
    useConversationActivityStore.setState(initialActivity, true)
    await i18n.changeLanguage('en-US')
  })

  it('shows nothing while a conversation is idle', () => {
    const { container } = render(<ConversationStatusIndicator conversationId="group-1" />)

    expect(container).toBeEmptyDOMElement()
  })

  it('names a running conversation even without visible words', () => {
    startRun('run-1', 'thread-1')

    render(<ConversationStatusIndicator conversationId="group-1" />)

    expect(screen.getByText('Replying')).toHaveClass('sr-only')
  })

  it('spells the status out where there is room for it', () => {
    startRun('run-1', 'thread-1')
    useConversationActivityStore.getState().markRunWaiting('run-1')

    render(<ConversationStatusIndicator conversationId="group-1" showLabel />)

    expect(screen.getByText('Waiting for you')).not.toHaveClass('sr-only')
  })

  it('scopes a task indicator to its own thread', () => {
    startRun('run-1', 'thread-1')

    render(
      <>
        <ThreadStatusIndicator conversationId="group-1" threadId="thread-1" showLabel />
        <ThreadStatusIndicator conversationId="group-1" threadId="thread-2" showLabel />
      </>,
    )

    expect(screen.getAllByText('Replying')).toHaveLength(1)
  })

  it('follows the conversation into a failure', () => {
    startRun('run-1', 'thread-1')
    useConversationActivityStore.getState().finishRun('run-1', 'failed', 'boom')

    render(<ConversationStatusIndicator conversationId="group-1" showLabel />)

    expect(screen.getByText('Reply failed')).toBeInTheDocument()
  })
})
