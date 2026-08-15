import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ToolApprovalCard } from '@/components/chat/ToolApprovalCard'
import i18n from '@/i18n'
import type { StreamApprovalRequest } from '@/stores/messageStore'

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

const request: StreamApprovalRequest = {
  tool_call_id: 'call_rm',
  rule: 'delete-files',
  capability: 'delete files in this workspace',
  reason: 'it deletes files, which cannot be undone from here.',
  tool_name: 'Pwsh',
  subject: 'rm -rf build',
}

describe('ToolApprovalCard', () => {
  it('shows the exact command being authorised rather than a paraphrase', () => {
    render(<ToolApprovalCard request={request} onAnswer={vi.fn()} />)

    // The whole point of the pause is that the user authorises *this* text, and
    // the runtime replays the same call.
    expect(screen.getByText('rm -rf build')).toBeVisible()
    expect(screen.getByText(/it deletes files/)).toBeVisible()
    expect(screen.getByText(/via Pwsh/)).toBeVisible()
  })

  it('offers a one-time allow, a thread-wide allow, and a decline', async () => {
    const user = userEvent.setup()
    const onAnswer = vi.fn()
    render(<ToolApprovalCard request={request} onAnswer={onAnswer} />)

    await user.click(screen.getByRole('button', { name: /allow once/i }))
    expect(onAnswer).toHaveBeenCalledWith({
      tool_call_id: 'call_rm',
      approved: true,
      remember: false,
      note: undefined,
    })
  })

  it('carries a note with a decline so the agent can try something else', async () => {
    const user = userEvent.setup()
    const onAnswer = vi.fn()
    render(<ToolApprovalCard request={request} onAnswer={onAnswer} />)

    await user.type(screen.getByLabelText(/note for the agent/i), 'I need those artifacts')
    await user.click(screen.getByRole('button', { name: /decline/i }))

    expect(onAnswer).toHaveBeenCalledWith({
      tool_call_id: 'call_rm',
      approved: false,
      remember: false,
      note: 'I need those artifacts',
    })
  })

  it('remembers the capability, not the command, when asked to', async () => {
    const user = userEvent.setup()
    const onAnswer = vi.fn()
    render(<ToolApprovalCard request={request} onAnswer={onAnswer} />)

    await user.click(screen.getByRole('button', { name: /allow for this conversation/i }))
    expect(onAnswer).toHaveBeenCalledWith(
      expect.objectContaining({ approved: true, remember: true }),
    )
  })

  it('stops offering buttons once answered, so a replayed card cannot run it again', () => {
    render(<ToolApprovalCard request={request} resolved="approved" onAnswer={vi.fn()} />)

    expect(screen.queryByRole('button')).toBeNull()
    expect(screen.getByText(/you allowed this command/i)).toBeVisible()
    // The record of what was authorised stays visible.
    expect(screen.getByText('rm -rf build')).toBeVisible()
  })

  it('is inert without a handler, so a card with no resumable thread cannot be clicked', async () => {
    const user = userEvent.setup()
    render(<ToolApprovalCard request={request} />)

    const allow = screen.getByRole('button', { name: /allow once/i })
    expect(allow).toBeDisabled()
    await user.click(allow)
    expect(screen.getByText('rm -rf build')).toBeVisible()
  })
})
