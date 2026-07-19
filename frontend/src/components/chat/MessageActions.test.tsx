import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MessageActions } from '@/components/chat/MessageActions'
import i18n from '@/i18n'

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({ data: [], isLoading: false }),
}))

vi.mock('@/hooks/useGroupMessages', () => ({
  useDeleteConversationMessage: () => ({ isPending: false, mutateAsync: vi.fn() }),
  useDeleteGroupMessage: () => ({ isPending: false, mutateAsync: vi.fn() }),
  useSendGroupMessage: () => ({ isPending: false, mutateAsync: vi.fn() }),
}))

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

describe('MessageActions', () => {
  it('uses the current language for the share dialog close button', async () => {
    const user = userEvent.setup()
    await i18n.changeLanguage('zh-CN')
    render(
      <MessageActions
        messageId="message-1"
        content="Raw message content"
        senderName="Agent"
        timeLabel="10:00"
        groupId="group-1"
      />,
    )

    await user.click(screen.getByRole('button', { name: '分享到群聊' }))
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })
})
