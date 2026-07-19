import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MessageAttachments } from '@/components/chat/MessageAttachments'
import type { MessageAttachment } from '@/types/api'

vi.mock('@/hooks/useGroupFiles', () => ({
  downloadGroupWorkspaceFile: vi.fn(),
}))

const attachments: MessageAttachment[] = [
  { id: 'image-1', path: 'uploads/photo.png', name: 'photo.png', mime_type: 'image/png', size: 1280, kind: 'image' },
  { id: 'file-1', path: 'uploads/report.pdf', name: 'report.pdf', mime_type: 'application/pdf', size: 2048, kind: 'file' },
]

afterEach(() => cleanup())

describe('MessageAttachments', () => {
  it('renders image previews and generic file metadata with open actions', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, blob: () => Promise.resolve(new Blob(['image'])) }))
    vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:photo'), revokeObjectURL: vi.fn() })
    render(<MessageAttachments groupId="group-1" attachments={attachments} />)

    expect(await screen.findByRole('img', { name: 'photo.png' })).toBeVisible()
    expect(screen.getByText('report.pdf')).toBeVisible()
    expect(screen.getByText('application/pdf')).toBeVisible()
    expect(screen.getByText('2 KB')).toBeVisible()
    expect(screen.queryByRole('img', { name: 'report.pdf' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open photo.png' })).toBeVisible()
    expect(screen.getByRole('button', { name: 'Open report.pdf' })).toBeVisible()
  })
})
