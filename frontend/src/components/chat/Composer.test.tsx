import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { Composer } from '@/components/chat/Composer'
import i18n from '@/i18n'
import type { GroupAgentRead } from '@/types/api'

const mocks = vi.hoisted(() => ({ upload: vi.fn() }))

vi.mock('@/hooks/useGroupFiles', () => ({
  WorkspaceUploadManyError: class WorkspaceUploadManyError extends Error {},
  useUploadGroupWorkspaceFiles: () => ({
    isPending: false,
    mutateAsync: mocks.upload,
  }),
}))

const groupAgents: GroupAgentRead[] = [
  {
    id: 'group-agent-1',
    group_id: 'group-1',
    agent_id: 'agent-1',
    display_name: 'Planner',
    role: null,
    topology_role: null,
    speaking_order: null,
    response_mode: 'default',
    share_group_workspace: false,
    context_usage: null,
    status: 'active',
    joined_at: '2026-07-18T00:00:00Z',
  },
]

describe('Composer mentions', () => {
  beforeEach(() => {
    mocks.upload.mockReset()
  })

  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('localizes the composer placeholder and stream cancellation action', async () => {
    await i18n.changeLanguage('en-US')
    render(<Composer onSend={vi.fn()} onCancel={vi.fn()} isStreaming />)
    expect(screen.getByPlaceholderText('Message your agents…')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Stop generating' })).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    render(<Composer onSend={vi.fn()} onCancel={vi.fn()} isStreaming />)
    expect(screen.getByPlaceholderText('给你的 Agent 发消息…')).toBeVisible()
    expect(screen.getByRole('button', { name: '停止生成' })).toBeVisible()
  })

  it('localizes upload error framing while preserving the raw detail', async () => {
    const user = userEvent.setup()
    mocks.upload.mockRejectedValueOnce(new Error('RAW_UPLOAD_DETAIL'))
    render(<Composer groupId="group-1" onSend={vi.fn()} />)

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      new File(['content'], 'notes.txt', { type: 'text/plain' }),
    )
    expect(await screen.findByText('Upload failed: RAW_UPLOAD_DETAIL')).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('上传失败：RAW_UPLOAD_DETAIL')).toBeVisible()
  })

  it.each([
    ['Tab', '{Tab}'],
    ['Enter', '{Enter}'],
    ['Space', ' '],
  ])('selects a filtered mention with %s', async (_label, key) => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    render(<Composer onSend={onSend} groupAgents={groupAgents} />)
    const textarea = screen.getByRole('textbox', { name: 'Message' })

    await user.type(textarea, '@pla')
    await user.keyboard(key)

    expect(textarea).toHaveValue('@Planner ')
    expect(onSend).not.toHaveBeenCalled()
  })

  it('summarizes large groups and reveals remaining agents on demand', async () => {
    const user = userEvent.setup()
    const agents = ['Planner', 'Researcher', 'Writer', 'Reviewer', 'Operator'].map(
      (display_name, index) => ({
        ...groupAgents[0],
        id: `group-agent-${index + 1}`,
        agent_id: `agent-${index + 1}`,
        display_name,
      }),
    )

    render(<Composer onSend={vi.fn()} groupAgents={agents} />)

    expect(screen.getByText('@Planner')).toBeVisible()
    expect(screen.getByText('@Researcher')).toBeVisible()
    expect(screen.getByText('@Writer')).toBeVisible()
    expect(screen.queryByText('@Reviewer')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Show 2 more agents' }))
    expect(screen.getByText('@Reviewer')).toBeVisible()
    expect(screen.getByText('@Operator')).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Close agent list' }))
    expect(screen.queryByText('@Reviewer')).not.toBeInTheDocument()
  })

  it('uploads an image and sends an attachment-only message with its workspace path', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce([{ path: 'uploads/photo.png' }])
    render(<Composer groupId="group-1" onSend={onSend} />)

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      new File(['png'], 'photo.png', { type: 'image/png' }),
    )
    await screen.findByText('photo.png')
    await user.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSend).toHaveBeenCalledWith({ content: '', attachments: [{ path: 'uploads/photo.png' }] })
  })

  it('uploads files dropped from the operating system', async () => {
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce([{ path: 'uploads/drop.png' }])
    render(<Composer groupId="group-1" onSend={onSend} />)
    const file = new File(['png'], 'drop.png', { type: 'image/png' })
    const textarea = screen.getByRole('textbox', { name: 'Message' })

    fireEvent.drop(textarea, { dataTransfer: { files: [file], types: ['Files'], getData: () => '' } })

    await waitFor(() => expect(mocks.upload).toHaveBeenCalledWith([file]))
  })

  it('uploads clipboard image files without preventing ordinary text paste', async () => {
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce([{ path: 'uploads/paste.webp' }])
    render(<Composer groupId="group-1" onSend={onSend} />)
    const file = new File(['webp'], 'paste.webp', { type: 'image/webp' })
    const textarea = screen.getByRole('textbox', { name: 'Message' })

    const imagePaste = fireEvent.paste(textarea, {
      clipboardData: { items: [{ kind: 'file', getAsFile: () => file }] },
    })
    await waitFor(() => expect(mocks.upload).toHaveBeenCalledWith([file]))
    expect(imagePaste).toBe(false)

    const textPaste = fireEvent.paste(textarea, {
      clipboardData: { items: [{ kind: 'string', getAsFile: () => null }] },
    })
    expect(textPaste).toBe(true)
  })

  it('keeps failed uploads removable and retryable without sending them', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    mocks.upload.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce([{ path: 'uploads/retry.pdf' }])
    render(<Composer groupId="group-1" onSend={onSend} />)

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      new File(['pdf'], 'retry.pdf', { type: 'application/pdf' }),
    )
    expect(await screen.findByText('offline')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()
    await user.click(screen.getByRole('button', { name: 'Retry upload retry.pdf' }))
    await screen.findByText('retry.pdf')
    await user.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSend).toHaveBeenCalledWith({ content: '', attachments: [{ path: 'uploads/retry.pdf' }] })
  })
})
