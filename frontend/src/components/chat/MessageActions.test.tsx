import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MessageActions } from '@/components/chat/MessageActions'
import i18n from '@/i18n'
import type { MessageAttachment } from '@/types/api'

const sendGroupMessage = vi.fn()
const fetchWorkspaceFileBlob = vi.fn()
const uploadWorkspaceFile = vi.fn()

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({
    data: [{ id: 'group-2', name: 'Design', description: null }],
    isLoading: false,
  }),
}))

vi.mock('@/hooks/useGroupMessages', () => ({
  useDeleteConversationMessage: () => ({ isPending: false, mutateAsync: vi.fn() }),
  useDeleteGroupMessage: () => ({ isPending: false, mutateAsync: vi.fn() }),
  useSendGroupMessage: () => ({ isPending: false, mutateAsync: sendGroupMessage }),
}))

vi.mock('@/hooks/useConversationWorkspaceFiles', () => ({
  fetchConversationWorkspaceFileBlob: (...args: unknown[]) => fetchWorkspaceFileBlob(...args),
  uploadConversationWorkspaceFile: (...args: unknown[]) => uploadWorkspaceFile(...args),
}))

const attachment: MessageAttachment = {
  id: 'attachment-1',
  path: 'uploads/photo.png',
  name: 'photo.png',
  mime_type: 'image/png',
  size: 1280,
  kind: 'image',
}

afterEach(async () => {
  cleanup()
  vi.clearAllMocks()
  vi.unstubAllGlobals()
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

  it('names the uploaded files in the copied text', async () => {
    const user = userEvent.setup()
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    render(
      <MessageActions
        messageId="message-1"
        content="Have a look"
        senderName="You"
        timeLabel="10:00"
        groupId="group-1"
        attachments={[attachment]}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Copy message' }))

    // Path as well as name: text pasted back into a composer still points at
    // the file, and a non-image attachment can never be anything else here.
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        'Have a look\n\nAttachments:\n- photo.png (uploads/photo.png)',
      ),
    )
  })

  it('puts an image attachment on the clipboard beside the text', async () => {
    const user = userEvent.setup()
    const write = vi.fn().mockResolvedValue(undefined)
    const blob = new Blob(['png'], { type: 'image/png' })
    fetchWorkspaceFileBlob.mockResolvedValue(blob)
    class FakeClipboardItem {
      constructor(readonly items: Record<string, unknown>) {}
    }
    vi.stubGlobal('ClipboardItem', FakeClipboardItem)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { write, writeText: vi.fn() } })
    render(
      <MessageActions
        messageId="message-1"
        content="Have a look"
        senderName="You"
        timeLabel="10:00"
        groupId="group-1"
        attachments={[attachment]}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Copy message' }))

    await waitFor(() => expect(write).toHaveBeenCalledTimes(1))
    const [[[item]]] = write.mock.calls as [[FakeClipboardItem[]]]
    expect(Object.keys(item.items)).toEqual(['text/plain', 'image/png'])
    await expect(item.items['image/png']).resolves.toBe(blob)
    expect(fetchWorkspaceFileBlob).toHaveBeenCalledWith(
      'groups',
      'group-1',
      'uploads/photo.png',
      null,
    )
  })

  it('falls back to text when the webview rejects the image flavour', async () => {
    const user = userEvent.setup()
    const write = vi.fn().mockRejectedValue(new Error('unsupported type'))
    const writeText = vi.fn().mockResolvedValue(undefined)
    fetchWorkspaceFileBlob.mockResolvedValue(new Blob(['png'], { type: 'image/png' }))
    vi.stubGlobal('ClipboardItem', class {})
    vi.stubGlobal('navigator', { ...navigator, clipboard: { write, writeText } })
    render(
      <MessageActions
        messageId="message-1"
        content="Have a look"
        senderName="You"
        timeLabel="10:00"
        groupId="group-1"
        attachments={[attachment]}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Copy message' }))

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        'Have a look\n\nAttachments:\n- photo.png (uploads/photo.png)',
      ),
    )
  })

  it('copies the files into the target group before sharing the message', async () => {
    const user = userEvent.setup()
    const blob = new Blob(['png'], { type: 'image/png' })
    fetchWorkspaceFileBlob.mockResolvedValue(blob)
    uploadWorkspaceFile.mockResolvedValue({ path: 'uploads/photo (1).png' })
    sendGroupMessage.mockResolvedValue({})
    render(
      <MessageActions
        messageId="message-1"
        content="Have a look"
        senderName="You"
        timeLabel="10:00"
        groupId="group-1"
        attachments={[attachment]}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Share message to group' }))
    await user.click(screen.getByRole('button', { name: /Design/ }))

    await waitFor(() => expect(sendGroupMessage).toHaveBeenCalledTimes(1))
    expect(fetchWorkspaceFileBlob).toHaveBeenCalledWith(
      'groups',
      'group-1',
      'uploads/photo.png',
      null,
    )
    // Uploaded under a free name in the *receiving* group: the send endpoint
    // validates every path against that group's own workspace.
    const [scope, targetGroupId, file, , agentScope, options] = uploadWorkspaceFile.mock.calls[0]
    expect([scope, targetGroupId, agentScope, options]).toEqual([
      'groups',
      'group-2',
      null,
      { uniqueName: true },
    ])
    expect((file as File).name).toBe('photo.png')
    expect(sendGroupMessage).toHaveBeenCalledWith({
      groupId: 'group-2',
      content: 'You · 10:00\n\nHave a look',
      attachments: [{ path: 'uploads/photo (1).png' }],
    })
  })

  it('reports a failed attachment copy instead of sharing the message without it', async () => {
    const user = userEvent.setup()
    fetchWorkspaceFileBlob.mockRejectedValue(new Error('file is gone'))
    render(
      <MessageActions
        messageId="message-1"
        content="Have a look"
        senderName="You"
        timeLabel="10:00"
        groupId="group-1"
        attachments={[attachment]}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Share message to group' }))
    await user.click(screen.getByRole('button', { name: /Design/ }))

    expect(await screen.findByText('Share failed: file is gone')).toBeVisible()
    expect(sendGroupMessage).not.toHaveBeenCalled()
  })
})
