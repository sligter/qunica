import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { Composer } from '@/components/chat/Composer'
import i18n from '@/i18n'
import { CONVERSATION_ID_MIME } from '@/lib/conversationDrag'
import { encodeWorkspaceDragItems, WORKSPACE_ITEM_MIME } from '@/lib/workspaceDrag'
import type {
  ConversationScope,
  ConversationWorkspaceFileRead,
  ConversationWorkspaceFileTextResponse,
  GroupAgentRead,
} from '@/types/api'

const mocks = vi.hoisted(() => ({
  getFile: vi.fn(),
  getMetadata: vi.fn(),
  upload: vi.fn(),
  uploadHook: vi.fn(),
}))

vi.mock('@/hooks/useConversationWorkspaceFiles', () => ({
  getConversationWorkspaceFile: mocks.getFile,
  getConversationWorkspaceFileMetadata: mocks.getMetadata,
  useUploadConversationWorkspaceFile: (...args: unknown[]) => {
    mocks.uploadHook(...args)
    return {
      isPending: false,
      mutateAsync: mocks.upload,
    }
  },
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
    workspace_mode: 'self', share_group_workspace: false,
    context_usage: null,
    status: 'active',
    joined_at: '2026-07-18T00:00:00Z',
  },
]

function workspaceFile(path: string, isDirectory = false): ConversationWorkspaceFileRead {
  const name = path.split('/').at(-1) ?? path
  return {
    path,
    name,
    is_dir: isDirectory,
    size: isDirectory ? null : 12,
    modified_at: '2026-07-25T00:00:00Z',
  }
}

function workspaceMetadata(
  path: string,
  mimeType = 'text/markdown',
  size = 12,
): ConversationWorkspaceFileTextResponse {
  return {
    path,
    name: path.split('/').at(-1) ?? path,
    mime_type: mimeType,
    size,
    content: mimeType.startsWith('text/') ? 'content' : null,
    is_text: mimeType.startsWith('text/'),
    truncated: false,
    version: 'version-1',
    message: null,
  }
}

function workspaceDataTransfer(
  items: Array<{ path: string; name?: string; kind: 'file' | 'directory' }>,
) {
  const encoded = encodeWorkspaceDragItems(items.map((item) => ({
    path: item.path,
    name: item.name ?? item.path.split('/').at(-1) ?? item.path,
    kind: item.kind,
  })))
  return {
    files: [],
    types: [WORKSPACE_ITEM_MIME],
    dropEffect: 'none',
    getData: (type: string) => type === WORKSPACE_ITEM_MIME ? encoded : '',
  }
}

function webViewWorkspaceDataTransfer(paths: string[]) {
  const text = paths.join('\n')
  return {
    files: [],
    types: ['text/plain'],
    dropEffect: 'none',
    getData: (type: string) => type === 'text/plain' ? text : '',
  }
}

function operatingSystemDataTransfer(files: File[]) {
  return {
    files,
    types: ['Files'],
    dropEffect: 'none',
    getData: () => '',
  }
}

function conversationDataTransfer(id: string) {
  return {
    files: [],
    types: [CONVERSATION_ID_MIME],
    dropEffect: 'none',
    getData: (type: string) => type === CONVERSATION_ID_MIME ? id : '',
  }
}

describe('Composer', () => {
  beforeEach(() => {
    localStorage.clear()
    mocks.getFile.mockReset()
    mocks.getMetadata.mockReset()
    mocks.upload.mockReset()
    mocks.uploadHook.mockReset()
  })

  afterEach(async () => {
    cleanup()
    vi.unstubAllGlobals()
    await i18n.changeLanguage('en-US')
  })

  it('never offers a per-message model choice', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    render(<Composer onSend={onSend} supportsReasoningEffort />)

    // The model is the agent's configuration. Letting a message carry its own
    // would silently split a conversation across two models.
    expect(screen.queryByRole('listbox')).toBeNull()
    await user.type(screen.getByRole('textbox'), 'hello')
    await user.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => expect(onSend).toHaveBeenCalled())
    expect(onSend.mock.calls[0][0]).not.toHaveProperty('model_override')
  })

  it('sends the chosen reasoning effort as a per-message override', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    render(<Composer onSend={onSend} supportsReasoningEffort />)

    fireEvent.change(screen.getByRole('slider', { name: /thinking/i }), {
      target: { value: '3' },
    })
    await user.type(screen.getByRole('textbox'), 'hard question')
    await user.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSend).toHaveBeenCalledWith(
        expect.objectContaining({ effort_override: 'high' }),
      ),
    )
  })

  it('hides the effort control for a model that does not support it', async () => {
    // Sending the field to a model that rejects it turns a normal question
    // into a provider error.
    render(<Composer onSend={vi.fn()} />)
    expect(screen.queryByRole('slider', { name: /thinking/i })).toBeNull()
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

  it('localizes upload errors without exposing backend diagnostics', async () => {
    const user = userEvent.setup()
    mocks.upload.mockRejectedValueOnce(new Error('RAW_UPLOAD_DETAIL'))
    render(<Composer groupId="group-1" onSend={vi.fn()} />)

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      new File(['content'], 'notes.txt', { type: 'text/plain' }),
    )
    expect(await screen.findByText(
      'Upload failed: The workspace operation could not be completed.',
    )).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('上传失败：无法完成此工作区操作。')).toBeVisible()
    expect(screen.queryByText('RAW_UPLOAD_DETAIL')).not.toBeInTheDocument()
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

  it.each([
    ['groups', 'group-1'],
    ['direct-chats', 'chat-1'],
  ] satisfies Array<[ConversationScope, string]>) (
    'adds and sends a server-confirmed workspace file in %s',
    async (scope, conversationId) => {
      const user = userEvent.setup()
      const onSend = vi.fn()
      mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
      render(
        <Composer
          conversationId={conversationId}
          workspaceId="workspace-1"
          scope={scope}
          onSend={onSend}
        />,
      )

      fireEvent.drop(screen.getByRole('group', { name: 'Message composer file drop area' }), {
        dataTransfer: workspaceDataTransfer([
          { path: 'docs/guide.md', name: 'guide.md', kind: 'file' },
        ]),
      })

      expect(await screen.findByText('text/markdown · 12 B')).toBeVisible()
      expect(mocks.getMetadata).toHaveBeenCalledWith(
        scope,
        conversationId,
        'docs/guide.md',
        null,
      )
      await user.click(screen.getByRole('button', { name: 'Send message' }))
      expect(onSend).toHaveBeenCalledWith({
        content: '',
        attachments: [{ path: 'docs/guide.md' }],
      })
    },
  )

  it('deduplicates repeated workspace file drops by server-confirmed path', async () => {
    mocks.getMetadata.mockResolvedValue(workspaceMetadata('docs/guide.md'))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={vi.fn()}
      />,
    )
    const dropZone = screen.getByRole('group', { name: 'Message composer file drop area' })
    const dataTransfer = workspaceDataTransfer([
      { path: 'docs/guide.md', name: 'guide.md', kind: 'file' },
      { path: 'docs/guide.md', name: 'guide.md', kind: 'file' },
    ])

    fireEvent.drop(dropZone, { dataTransfer })
    expect(await screen.findByText('guide.md')).toBeVisible()
    fireEvent.drop(dropZone, { dataTransfer })

    await waitFor(() => expect(screen.getAllByText('guide.md')).toHaveLength(1))
    expect(mocks.getMetadata).toHaveBeenCalledTimes(1)
  })

  it('attaches a server-confirmed file from a text/plain-only WebView drop', async () => {
    mocks.getFile.mockResolvedValueOnce(workspaceFile('docs/guide.md'))
    mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
    render(
      <Composer
        conversationId="chat-1"
        workspaceId="workspace-1"
        scope="direct-chats"
        onSend={vi.fn()}
      />,
    )
    const dropZone = screen.getByRole('group', {
      name: 'Message composer file drop area',
    })
    const dataTransfer = webViewWorkspaceDataTransfer(['docs/guide.md'])

    fireEvent.dragOver(dropZone, { dataTransfer })
    fireEvent.drop(dropZone, { dataTransfer })

    expect(await screen.findByText('guide.md')).toBeVisible()
    expect(mocks.getFile).toHaveBeenCalledWith(
      'direct-chats',
      'chat-1',
      'docs/guide.md',
      null,
    )
  })

  it('inserts a server-confirmed directory from a text/plain-only WebView drop', async () => {
    const user = userEvent.setup()
    mocks.getFile.mockResolvedValueOnce(workspaceFile('docs', true))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={vi.fn()}
      />,
    )
    const textarea = screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement
    await user.type(textarea, 'open OLD now')
    textarea.setSelectionRange(5, 8)

    fireEvent.drop(
      screen.getByRole('group', { name: 'Message composer file drop area' }),
      { dataTransfer: webViewWorkspaceDataTransfer(['docs']) },
    )

    await waitFor(() => expect(textarea).toHaveValue('open docs now'))
  })

  it('replaces the focused selection with a directory path and appends when unfocused', async () => {
    const user = userEvent.setup()
    mocks.getFile
      .mockResolvedValueOnce(workspaceFile('docs', true))
      .mockResolvedValueOnce(workspaceFile('assets', true))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={vi.fn()}
      />,
    )
    const textarea = screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement
    const dropZone = screen.getByRole('group', { name: 'Message composer file drop area' })
    await user.type(textarea, 'open OLD now')
    textarea.focus()
    textarea.setSelectionRange(5, 8)

    fireEvent.drop(dropZone, {
      dataTransfer: workspaceDataTransfer([{ path: 'docs', kind: 'directory' }]),
    })
    await waitFor(() => expect(textarea).toHaveValue('open docs now'))
    await waitFor(() => {
      expect(textarea).toHaveFocus()
      expect(textarea).toHaveProperty('selectionStart', 9)
    })

    textarea.blur()
    fireEvent.drop(dropZone, {
      dataTransfer: workspaceDataTransfer([{ path: 'assets', kind: 'directory' }]),
    })
    await waitFor(() => expect(textarea).toHaveValue('open docs now assets'))
    await waitFor(() => {
      expect(textarea).toHaveFocus()
      expect(textarea).toHaveProperty('selectionStart', 20)
    })
  })

  it('keeps the card highlighted until the final nested dragleave', () => {
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={vi.fn()}
      />,
    )
    const dropZone = screen.getByRole('group', { name: 'Message composer file drop area' })
    const dataTransfer = workspaceDataTransfer([{ path: 'docs', kind: 'directory' }])

    fireEvent.dragEnter(dropZone, { dataTransfer })
    fireEvent.dragEnter(screen.getByRole('textbox', { name: 'Message' }), { dataTransfer })
    expect(dropZone).toHaveAttribute('data-drop-active', 'true')
    fireEvent.dragLeave(screen.getByRole('textbox', { name: 'Message' }), { dataTransfer })
    expect(dropZone).toHaveAttribute('data-drop-active', 'true')
    fireEvent.dragLeave(dropZone, { dataTransfer })
    expect(dropZone).toHaveAttribute('data-drop-active', 'false')
  })

  it('announces drop readiness and completion to assistive technology', async () => {
    mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={vi.fn()}
      />,
    )
    const dropZone = screen.getByRole('group', { name: 'Message composer file drop area' })
    const textarea = screen.getByRole('textbox', { name: 'Message' })
    const dataTransfer = workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }])

    fireEvent.dragEnter(dropZone, { dataTransfer })
    expect(textarea).toHaveAccessibleDescription(
      'Drop workspace files to attach them or folders to insert their paths.',
    )

    fireEvent.drop(dropZone, { dataTransfer })
    await waitFor(() => expect(textarea).toHaveAccessibleDescription('Workspace file added.'))
  })

  it('enhances an unchanged draft with the selected group member', async () => {
    const user = userEvent.setup()
    const onEnhance = vi.fn().mockResolvedValue('Implement the current group task.')
    const enhanceAgents = [
      { ...groupAgents[0], prompt_enhancement_available: true },
      {
        ...groupAgents[0],
        id: 'group-agent-2',
        agent_id: 'agent-2',
        display_name: 'Reviewer',
        prompt_enhancement_available: true,
      },
    ]
    render(
      <Composer
        onSend={vi.fn()}
        onEnhance={onEnhance}
        enhanceAgents={enhanceAgents}
      />,
    )

    const textarea = screen.getByRole('textbox', { name: 'Message' })
    await user.type(textarea, 'do it')
    // The member list is a menu rather than a permanent select: the toolbar
    // has room for icons, not for a 140px dropdown.
    expect(screen.queryByText('Reviewer')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Enhancement member' }))
    await user.click(screen.getByRole('option', { name: 'Reviewer' }))
    expect(screen.queryByRole('listbox')).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Enhance with group context' }))

    await waitFor(() => expect(textarea).toHaveValue('Implement the current group task.'))
    expect(onEnhance).toHaveBeenCalledWith('do it', 'agent-2')
  })

  it('restores each conversation draft until it is sent', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn().mockResolvedValue(undefined)
    const first = render(<Composer draftKey="direct-chats:chat-1" onSend={onSend} />)

    await user.type(screen.getByRole('textbox', { name: 'Message' }), 'keep this')
    first.unmount()
    const other = render(<Composer draftKey="direct-chats:chat-2" onSend={onSend} />)
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('')

    other.unmount()
    const restored = render(<Composer draftKey="direct-chats:chat-1" onSend={onSend} />)
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('keep this')
    await user.click(screen.getByRole('button', { name: 'Send message' }))
    restored.unmount()

    render(<Composer draftKey="direct-chats:chat-1" onSend={onSend} />)
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('')
  })

  it('allows vertical resizing without undoing it on the next keystroke', () => {
    render(<Composer onSend={vi.fn()} />)
    const textarea = screen.getByRole('textbox', { name: 'Message' })
    const grip = screen.getByRole('button', { name: 'Resize the message box' })
    // The native handle drew itself between the text and the toolbar rather
    // than on an edge, so height is driven from the grip instead.
    expect(textarea).toHaveClass('resize-none', 'max-h-[50vh]')

    fireEvent.pointerDown(grip, { button: 0, clientY: 500 })
    fireEvent.pointerMove(window, { clientY: 400 })
    fireEvent.pointerUp(window)
    // Dragging the grip up grows the box: the composer is pinned to the bottom.
    expect(textarea).toHaveStyle({ height: '140px' })

    fireEvent.change(textarea, { target: { value: 'after resize' } })
    expect(textarea).toHaveStyle({ height: '140px' })
  })

  it('returns a hand-resized box to fitting the text on a double-click', () => {
    render(<Composer onSend={vi.fn()} />)
    const textarea = screen.getByRole('textbox', { name: 'Message' })
    const grip = screen.getByRole('button', { name: 'Resize the message box' })

    fireEvent.pointerDown(grip, { button: 0, clientY: 500 })
    fireEvent.pointerMove(window, { clientY: 300 })
    fireEvent.pointerUp(window)
    expect(textarea).toHaveStyle({ height: '240px' })

    fireEvent.doubleClick(grip)
    expect(textarea).not.toHaveStyle({ height: '240px' })
  })

  it('never lets a drag grow the box past half the window', () => {
    render(<Composer onSend={vi.fn()} />)
    const textarea = screen.getByRole('textbox', { name: 'Message' })
    const grip = screen.getByRole('button', { name: 'Resize the message box' })

    fireEvent.pointerDown(grip, { button: 0, clientY: 500 })
    fireEvent.pointerMove(window, { clientY: -10_000 })
    fireEvent.pointerUp(window)

    expect(textarea).toHaveStyle({ height: `${Math.round(window.innerHeight * 0.5)}px` })
  })

  it('inserts a sidebar conversation ID in the Assistant composer', async () => {
    render(
      <Composer
        conversationId="assistant-chat"
        workspaceId={null}
        scope="direct-chats"
        allowConversationDrop
        onSend={vi.fn()}
      />,
    )
    const dropZone = screen.getByRole('group', { name: 'Message composer file drop area' })
    const textarea = screen.getByRole('textbox', { name: 'Message' })
    const dataTransfer = conversationDataTransfer('chat-123')

    fireEvent.dragEnter(dropZone, { dataTransfer })
    expect(textarea).toHaveAccessibleDescription('Drop to insert this conversation ID.')
    fireEvent.drop(dropZone, { dataTransfer })

    expect(textarea).toHaveValue('conversation_id: chat-123')
    expect(textarea).toHaveAccessibleDescription('Conversation ID inserted.')
    expect(mocks.getFile).not.toHaveBeenCalled()
  })

  it('uploads an image and sends an attachment-only message with its workspace path', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce({ path: 'uploads/photo.png' })
    render(<Composer groupId="group-1" onSend={onSend} />)

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      new File(['png'], 'photo.png', { type: 'image/png' }),
    )
    await screen.findByText('photo.png')
    await user.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSend).toHaveBeenCalledWith({ content: '', attachments: [{ path: 'uploads/photo.png' }] })
    await waitFor(() => expect(screen.queryByText('photo.png')).not.toBeInTheDocument())
  })

  it('opens a preview when a pasted image attachment thumbnail is clicked', async () => {
    const user = userEvent.setup()
    mocks.upload.mockResolvedValueOnce({ path: 'uploads/paste.png' })
    vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:paste'), revokeObjectURL: vi.fn() })
    render(<Composer groupId="group-1" onSend={vi.fn()} />)

    const file = new File(['png'], 'paste.png', { type: 'image/png' })
    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      file,
    )
    await user.click(await screen.findByRole('button', { name: 'Preview paste.png' }))

    expect(screen.getByRole('dialog')).toBeVisible()
    expect(screen.getByRole('img', { name: 'paste.png' })).toHaveAttribute('src', 'blob:paste')
  })

  it('uploads files dropped from the operating system', async () => {
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce({ path: 'uploads/drop.png' })
    render(<Composer groupId="group-1" onSend={onSend} />)
    const file = new File(['png'], 'drop.png', { type: 'image/png' })
    const textarea = screen.getByRole('textbox', { name: 'Message' })

    fireEvent.drop(textarea, { dataTransfer: operatingSystemDataTransfer([file]) })

    await waitFor(() => expect(mocks.upload).toHaveBeenCalledWith(file))
  })

  it('uploads and sends files in direct chats', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce({ path: 'uploads/notes.txt' })
    render(
      <Composer
        conversationId="chat-1"
        workspaceId="workspace-1"
        scope="direct-chats"
        onSend={onSend}
      />,
    )
    const file = new File(['text'], 'notes.txt', { type: 'text/plain' })

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      file,
    )
    await screen.findByText('notes.txt')
    expect(mocks.uploadHook).toHaveBeenCalledWith('direct-chats', 'chat-1', null, {
      uniqueName: true,
    })
    expect(mocks.upload).toHaveBeenCalledWith(file)

    await user.click(screen.getByRole('button', { name: 'Send message' }))
    expect(onSend).toHaveBeenCalledWith({
      content: '',
      attachments: [{ path: 'uploads/notes.txt' }],
    })
  })

  it('rejects drops while disabled without reading metadata or uploading files', () => {
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        disabledReason="Read only"
        onSend={vi.fn()}
      />,
    )
    const dropZone = screen.getByRole('group', { name: 'Message composer file drop area' })
    fireEvent.dragEnter(dropZone, {
      dataTransfer: workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }]),
    })
    fireEvent.drop(dropZone, {
      dataTransfer: workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }]),
    })
    fireEvent.drop(dropZone, {
      dataTransfer: operatingSystemDataTransfer([
        new File(['text'], 'notes.txt', { type: 'text/plain' }),
      ]),
    })

    expect(dropZone).toHaveAttribute('data-drop-active', 'false')
    expect(mocks.getMetadata).not.toHaveBeenCalled()
    expect(mocks.upload).not.toHaveBeenCalled()
  })

  it('reports a missing workspace without accepting workspace references', async () => {
    render(
      <Composer
        conversationId="chat-1"
        workspaceId={null}
        scope="direct-chats"
        onSend={vi.fn()}
      />,
    )

    expect(
      screen.queryByRole('button', { name: 'Upload files to workspace uploads' }),
    ).toBeNull()

    fireEvent.drop(screen.getByRole('group', { name: 'Message composer file drop area' }), {
      dataTransfer: workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }]),
    })

    expect(await screen.findByText('This conversation has no local workspace.')).toBeVisible()
    expect(mocks.getMetadata).not.toHaveBeenCalled()
  })

  it('removes a workspace attachment from the draft without any disk mutation', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={onSend}
      />,
    )
    fireEvent.drop(screen.getByRole('group', { name: 'Message composer file drop area' }), {
      dataTransfer: workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }]),
    })

    await user.click(await screen.findByRole('button', { name: 'Remove guide.md' }))
    expect(screen.queryByText('guide.md')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()
    expect(onSend).not.toHaveBeenCalled()
  })

  it('clears ready attachments as soon as an async send starts', async () => {
    const user = userEvent.setup()
    let resolveSend!: () => void
    const onSend = vi.fn(() => new Promise<void>((resolve) => {
      resolveSend = resolve
    }))
    mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={onSend}
      />,
    )
    fireEvent.drop(screen.getByRole('group', { name: 'Message composer file drop area' }), {
      dataTransfer: workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }]),
    })
    await screen.findByText('guide.md')

    // The composer empties on submit rather than on acknowledgement: the
    // conversation already echoes the message, and holding the draft for the
    // round trip is what made the first message of a chat feel frozen.
    await user.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() => expect(screen.queryByText('guide.md')).not.toBeInTheDocument())
    await act(async () => resolveSend())
    expect(screen.queryByText('guide.md')).not.toBeInTheDocument()
  })

  it('restores ready attachments when an async send fails', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn().mockRejectedValue(new Error('send failed'))
    mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
    render(
      <Composer
        conversationId="group-1"
        workspaceId="workspace-1"
        scope="groups"
        onSend={onSend}
      />,
    )
    fireEvent.drop(screen.getByRole('group', { name: 'Message composer file drop area' }), {
      dataTransfer: workspaceDataTransfer([{ path: 'docs/guide.md', kind: 'file' }]),
    })
    await screen.findByText('guide.md')

    await user.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Send message' })).toBeEnabled())
    expect(screen.getByText('guide.md')).toBeVisible()
    expect(onSend).toHaveBeenCalledWith({
      content: '',
      attachments: [{ path: 'docs/guide.md' }],
    })
  })

  it('uploads clipboard image files without preventing ordinary text paste', async () => {
    const onSend = vi.fn()
    mocks.upload.mockResolvedValueOnce({ path: 'uploads/paste.webp' })
    render(<Composer groupId="group-1" onSend={onSend} />)
    const file = new File(['webp'], 'paste.webp', { type: 'image/webp' })
    const textarea = screen.getByRole('textbox', { name: 'Message' })

    const imagePaste = fireEvent.paste(textarea, {
      clipboardData: { items: [{ kind: 'file', getAsFile: () => file }] },
    })
    await waitFor(() => expect(mocks.upload).toHaveBeenCalledWith(file))
    expect(imagePaste).toBe(false)

    const textPaste = fireEvent.paste(textarea, {
      clipboardData: { items: [{ kind: 'string', getAsFile: () => null }] },
    })
    expect(textPaste).toBe(true)
  })

  it('keeps failed uploads removable and retryable without sending them', async () => {
    const user = userEvent.setup()
    const onSend = vi.fn()
    mocks.upload.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ path: 'uploads/retry.pdf' })
    render(<Composer groupId="group-1" onSend={onSend} />)

    await user.upload(
      screen.getByLabelText('Upload files to workspace uploads', { selector: 'input' }),
      new File(['pdf'], 'retry.pdf', { type: 'application/pdf' }),
    )
    expect(await screen.findByText('Upload failed: Unable to reach the workspace service.')).toBeVisible()
    expect(screen.getAllByText('Upload failed')).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()
    await user.click(screen.getByRole('button', { name: 'Retry upload retry.pdf' }))
    await screen.findByText('retry.pdf')
    await user.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSend).toHaveBeenCalledWith({ content: '', attachments: [{ path: 'uploads/retry.pdf' }] })
  })
})
