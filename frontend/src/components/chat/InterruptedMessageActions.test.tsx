import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { InterruptedMessageActions } from '@/components/chat/InterruptedMessageActions'
import i18n from '@/i18n'
import { useMessageStore } from '@/stores/messageStore'
import type { MessageToolCall } from '@/types/api'

const mocks = vi.hoisted(() => ({ resume: vi.fn() }))

vi.mock('@/hooks/useResumeStream', () => ({
  useResumeStream: () => ({
    resume: mocks.resume,
    cancel: vi.fn(),
    isStreaming: false,
    error: null,
    retry: null,
    retryExhausted: false,
  }),
}))

const initialMessages = useMessageStore.getInitialState()

function toolCall(overrides: Partial<MessageToolCall>): MessageToolCall {
  return {
    tool_call_id: 'call_rm',
    tool_name: 'Pwsh',
    status: 'approval_required',
    args_summary: '{"command":"rm -rf build"}',
    result_summary: null,
    approval_request: {
      rule: 'delete-files',
      capability: 'delete files in this workspace',
      reason: 'it deletes files, which cannot be undone from here.',
      tool_name: 'Pwsh',
      subject: 'rm -rf build',
    },
    ...overrides,
  }
}

function renderActions(toolCalls: MessageToolCall[] | null) {
  return render(
    <InterruptedMessageActions
      groupId="group-1"
      stateId="thread-1"
      messageId="message-1"
      toolCalls={toolCalls}
    />,
  )
}

describe('InterruptedMessageActions', () => {
  beforeEach(() => {
    mocks.resume.mockReset()
    useMessageStore.setState(initialMessages, true)
  })

  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('offers only continue when the turn was interrupted rather than gated', () => {
    renderActions([toolCall({ status: 'completed', result_summary: 'ok' })])

    expect(screen.getByRole('button', { name: /continue/i })).toBeVisible()
    expect(screen.queryByRole('button', { name: /allow once/i })).toBeNull()
  })

  it('rebuilds the card from the checkpoint so a reload can still answer', async () => {
    const user = userEvent.setup()
    // No live stream anywhere: this is what is left after the app restarts.
    renderActions([toolCall({ tool_call_id: 'call_done', status: 'completed', result_summary: 'ok' }), toolCall({})])

    expect(screen.getByText('rm -rf build')).toBeVisible()
    // Continue would only invite the model to propose the command again, so the
    // card replaces it rather than sitting beside it.
    expect(screen.queryByRole('button', { name: /continue/i })).toBeNull()

    await user.click(screen.getByRole('button', { name: /allow once/i }))
    expect(mocks.resume).toHaveBeenCalledWith({
      tool_call_id: 'call_rm',
      approved: true,
      remember: false,
      note: undefined,
    })
  })

  it('stands down while the live timeline is already offering the same card', () => {
    useMessageStore.getState().appendStreamNotice('thread-1', 'stream-1', {
      type: 'approval_required',
      message: 'Approval required',
      approval_request: {
        tool_call_id: 'call_rm',
        rule: 'delete-files',
        capability: 'delete files in this workspace',
        reason: 'it deletes files',
        tool_name: 'Pwsh',
        subject: 'rm -rf build',
      },
    })

    renderActions([toolCall({})])

    // One pause must not produce two answerable cards.
    expect(screen.queryByRole('button', { name: /allow once/i })).toBeNull()
  })

  it('ignores a gated call that has already been answered', () => {
    renderActions([toolCall({ status: 'approval_required', result_summary: 'declined' })])

    expect(screen.queryByRole('button', { name: /allow once/i })).toBeNull()
    expect(screen.getByRole('button', { name: /continue/i })).toBeVisible()
  })
})
