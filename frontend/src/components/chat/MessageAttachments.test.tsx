import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { MessageAttachments } from '@/components/chat/MessageAttachments'
import type { MessageAttachment } from '@/types/api'

const mocks = vi.hoisted(() => ({
  fetchBlob: vi.fn(),
  download: vi.fn(),
}))

vi.mock('@/hooks/useConversationWorkspaceFiles', () => ({
  fetchConversationWorkspaceFileBlob: mocks.fetchBlob,
  downloadConversationWorkspaceFile: mocks.download,
}))

const attachments: MessageAttachment[] = [
  { id: 'image-1', path: 'uploads/photo.png', name: 'photo.png', mime_type: 'image/png', size: 1280, kind: 'image' },
  { id: 'file-1', path: 'uploads/report.pdf', name: 'report.pdf', mime_type: 'application/pdf', size: 2048, kind: 'file' },
]

beforeEach(() => {
  mocks.fetchBlob.mockReset().mockResolvedValue(new Blob(['image']))
  mocks.download.mockReset().mockResolvedValue(undefined)
})

afterEach(() => cleanup())

describe('MessageAttachments', () => {
  it('renders image previews and generic file metadata with open actions', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:photo'), revokeObjectURL: vi.fn() })
    render(<MessageAttachments groupId="chat-1" scope="direct-chats" attachments={attachments} />)

    expect(await screen.findByRole('img', { name: 'photo.png' })).toBeVisible()
    expect(screen.getByText('report.pdf')).toBeVisible()
    expect(screen.getByText('application/pdf')).toBeVisible()
    expect(screen.getByText('2 KB')).toBeVisible()
    expect(screen.queryByRole('img', { name: 'report.pdf' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open photo.png' })).toBeVisible()
    expect(screen.getByRole('button', { name: 'Open report.pdf' })).toBeVisible()
    expect(mocks.fetchBlob).toHaveBeenCalledWith('direct-chats', 'chat-1', 'uploads/photo.png', null)

    await user.click(screen.getByRole('button', { name: 'Open report.pdf' }))
    expect(mocks.download).toHaveBeenCalledWith('direct-chats', 'chat-1', 'uploads/report.pdf', null)
  })

  it('opens an image lightbox when a message image is clicked', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:photo'), revokeObjectURL: vi.fn() })
    render(<MessageAttachments groupId="group-1" attachments={attachments} />)

    await user.click(await screen.findByRole('button', { name: 'Preview photo.png' }))

    const dialog = screen.getByRole('dialog')
    expect(dialog).toBeVisible()
    expect(screen.getByRole('img', { name: 'photo.png' })).toBeVisible()
  })
})
