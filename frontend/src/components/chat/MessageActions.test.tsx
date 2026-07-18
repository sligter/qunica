import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { MessageActions } from '@/components/chat/MessageActions'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  share: vi.fn(),
  remove: vi.fn(),
}))

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({
    data: [{ id: 'group-2', name: 'Target group', description: null }],
    isLoading: false,
  }),
}))

vi.mock('@/hooks/useGroupMessages', () => ({
  useSendGroupMessage: () => ({ isPending: false, mutateAsync: mocks.share }),
  useDeleteGroupMessage: () => ({ isPending: false, mutateAsync: mocks.remove }),
}))

function renderActions() {
  return render(
    <MessageActions
      messageId="message-1"
      content="RAW_MESSAGE_CONTENT"
      senderName="Researcher"
      timeLabel="10:00"
      groupId="group-1"
    />,
  )
}

describe('MessageActions localized errors', () => {
  beforeEach(() => {
    mocks.share.mockReset()
    mocks.remove.mockReset()
  })

  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('localizes share failure framing while preserving the raw detail', async () => {
    const user = userEvent.setup()
    mocks.share.mockRejectedValueOnce(new Error('RAW_SHARE_DETAIL'))
    renderActions()

    await user.click(screen.getByRole('button', { name: 'Share message to group' }))
    await user.click(screen.getByRole('button', { name: 'Target group' }))
    expect(await screen.findByText('Share failed: RAW_SHARE_DETAIL')).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('分享失败：RAW_SHARE_DETAIL')).toBeVisible()
  })

  it('localizes delete failure framing while preserving the raw detail', async () => {
    const user = userEvent.setup()
    mocks.remove.mockRejectedValueOnce(new Error('RAW_DELETE_DETAIL'))
    renderActions()

    await user.click(screen.getByRole('button', { name: 'Delete message' }))
    expect(await screen.findByText('Delete failed: RAW_DELETE_DETAIL')).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('删除失败：RAW_DELETE_DETAIL')).toBeVisible()
  })
})
