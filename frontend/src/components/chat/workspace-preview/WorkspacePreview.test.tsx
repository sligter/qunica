import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspacePreviewRouter } from '@/components/chat/workspace-preview/WorkspacePreviewRouter'
import { WorkspaceTextEditor } from '@/components/chat/workspace-preview/WorkspaceTextEditor'
import {
  conversationWorkspaceFileBlobQueryKey,
  conversationWorkspaceFileTextQueryKey,
} from '@/hooks/useConversationWorkspaceFiles'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import type {
  ConversationWorkspaceFileRead,
  ConversationWorkspaceFileTextResponse,
} from '@/types/api'

const baseFile: ConversationWorkspaceFileRead = {
  path: 'docs/note.txt',
  name: 'note.txt',
  is_dir: false,
  size: 5,
  modified_at: '2026-07-25T00:00:00Z',
}

function textResponse(
  overrides: Partial<ConversationWorkspaceFileTextResponse> = {},
): ConversationWorkspaceFileTextResponse {
  return {
    path: baseFile.path,
    name: baseFile.name,
    mime_type: 'text/plain',
    size: 5,
    content: 'hello',
    is_text: true,
    truncated: false,
    version: 'version-1',
    message: null,
    ...overrides,
  }
}

function testClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderRouter(
  client: QueryClient,
  file: ConversationWorkspaceFileRead,
  text: ConversationWorkspaceFileTextResponse,
  blob?: Blob,
  agentId: string | null = null,
  presentation: 'dialog' | 'editor' = 'dialog',
) {
  client.setQueryData(
    conversationWorkspaceFileTextQueryKey('groups', 'group-1', file.path, agentId),
    text,
  )
  if (blob) {
    client.setQueryData(
      conversationWorkspaceFileBlobQueryKey('groups', 'group-1', file.path, agentId),
      blob,
    )
  }
  return render(
    <QueryClientProvider client={client}>
      <WorkspacePreviewRouter
        scope="groups"
        conversationId="group-1"
        file={file}
        agentId={agentId}
        presentation={presentation}
      />
    </QueryClientProvider>,
  )
}

describe('WorkspacePreviewRouter secure Blob previews', () => {
  beforeEach(async () => {
    useAuthStore.setState({ token: null })
    await i18n.changeLanguage('en-US')
  })

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  it('uses an empty HTML sandbox and revokes Object URLs on switch and unmount', async () => {
    const createObjectURL = vi.fn()
      .mockReturnValueOnce('blob:html-preview')
      .mockReturnValueOnce('blob:pdf-preview')
    const revokeObjectURL = vi.fn()
    vi.stubGlobal('URL', { createObjectURL, revokeObjectURL })
    const client = testClient()
    const htmlFile = { ...baseFile, path: 'page.html', name: 'page.html' }
    const htmlText = textResponse({
      path: htmlFile.path,
      name: htmlFile.name,
      mime_type: 'text/html',
      content: '<script>window.top.location="https://example.com"</script>',
    })
    const view = renderRouter(
      client,
      htmlFile,
      htmlText,
      new Blob([htmlText.content ?? ''], { type: 'text/html' }),
    )

    const iframe = await screen.findByTitle('Sandboxed HTML preview of page.html')
    expect(iframe).toHaveAttribute('src', 'blob:html-preview')
    expect(iframe.getAttribute('sandbox')).toBe('')
    expect(iframe).not.toHaveAttribute('allow')

    const pdfFile = { ...baseFile, path: 'fake.html', name: 'fake.html' }
    const pdfText = textResponse({
      path: pdfFile.path,
      name: pdfFile.name,
      mime_type: 'application/pdf',
      content: null,
      is_text: false,
    })
    client.setQueryData(
      conversationWorkspaceFileTextQueryKey('groups', 'group-1', pdfFile.path),
      pdfText,
    )
    client.setQueryData(
      conversationWorkspaceFileBlobQueryKey('groups', 'group-1', pdfFile.path),
      new Blob(['pdf'], { type: 'application/pdf' }),
    )
    view.rerender(
      <QueryClientProvider client={client}>
        <WorkspacePreviewRouter scope="groups" conversationId="group-1" file={pdfFile} />
      </QueryClientProvider>,
    )

    expect(await screen.findByTitle('PDF preview of fake.html')).toHaveAttribute(
      'data',
      'blob:pdf-preview',
    )
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:html-preview')

    view.unmount()
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:pdf-preview')
    expect(client.getQueryData(
      conversationWorkspaceFileBlobQueryKey('groups', 'group-1', pdfFile.path),
    )).toBeUndefined()
  })

  it('routes server-confirmed text to the editor even when the filename looks binary', () => {
    const client = testClient()
    const textFile = { ...baseFile, path: 'notes/photo.png', name: 'photo.png' }
    renderRouter(client, textFile, textResponse({
      path: textFile.path,
      name: textFile.name,
      mime_type: 'text/plain',
      is_text: true,
    }))

    expect(screen.getByRole('textbox', { name: 'Edit photo.png' })).toHaveValue('hello')
    expect(document.querySelector('[data-preview-kind="text"]')).not.toBeNull()
  })

  it('loads HTML text and Blob data from the selected agent root', async () => {
    vi.stubGlobal('URL', {
      createObjectURL: vi.fn(() => 'blob:agent-html-preview'),
      revokeObjectURL: vi.fn(),
    })
    const client = testClient()
    const htmlFile = { ...baseFile, path: 'index.html', name: 'index.html' }
    const htmlText = textResponse({
      path: htmlFile.path,
      name: htmlFile.name,
      mime_type: 'text/html',
      content: '<h1>Agent workspace</h1>',
    })

    renderRouter(
      client,
      htmlFile,
      htmlText,
      new Blob([htmlText.content ?? ''], { type: 'text/html' }),
      'agent-1',
    )

    expect(await screen.findByTitle('Sandboxed HTML preview of index.html')).toHaveAttribute(
      'src',
      'blob:agent-html-preview',
    )
  })

  it('previews Markdown in dialogs and edits its source in editor tabs', () => {
    const client = testClient()
    const markdownFile = { ...baseFile, path: 'README.md', name: 'README.md' }
    const markdown = textResponse({
      path: markdownFile.path,
      name: markdownFile.name,
      mime_type: 'text/markdown',
      content: '# Preview title\n\n- rendered item',
    })
    const dialog = renderRouter(client, markdownFile, markdown)

    expect(screen.getByRole('heading', { name: 'Preview title' })).toBeVisible()
    expect(screen.getByText('rendered item')).toBeVisible()
    expect(document.querySelector('[data-preview-kind="markdown"]')).not.toBeNull()
    expect(screen.queryByRole('textbox')).toBeNull()

    dialog.unmount()
    renderRouter(client, markdownFile, markdown, undefined, null, 'editor')
    expect(screen.getByRole('textbox', { name: 'Edit README.md' })).toHaveValue(
      '# Preview title\n\n- rendered item',
    )
    expect(document.querySelector('[data-preview-kind="text"]')).not.toBeNull()
  })

  it('routes images to a bounded lightbox preview and falls back on load failure', async () => {
    const revokeObjectURL = vi.fn()
    vi.stubGlobal('URL', {
      createObjectURL: vi.fn(() => 'blob:image-preview'),
      revokeObjectURL,
    })
    const client = testClient()
    const imageFile = { ...baseFile, path: 'photo.png', name: 'photo.png', size: 128 }
    renderRouter(
      client,
      imageFile,
      textResponse({
        path: imageFile.path,
        name: imageFile.name,
        mime_type: 'image/png',
        content: null,
        is_text: false,
        size: 128,
      }),
      new Blob(['png'], { type: 'image/png' }),
    )

    const open = await screen.findByRole('button', {
      name: 'Open full-size preview of photo.png',
    })
    expect(open.querySelector('img')).toHaveClass('max-h-[55vh]')
    fireEvent.error(screen.getByRole('img', { name: 'photo.png' }))

    expect(await screen.findByText(
      'The image could not be displayed. You can still download the original file.',
    )).toBeVisible()
    expect(screen.getByRole('button', { name: 'Download file' })).toBeVisible()
    await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith('blob:image-preview'))
  })

  it('shows Office files as metadata and keeps keyboard download available', async () => {
    const user = userEvent.setup()
    const fetchMock = vi.fn().mockResolvedValue(new Response('docx', { status: 200 }))
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined)
    vi.stubGlobal('fetch', fetchMock)
    vi.stubGlobal('URL', {
      createObjectURL: vi.fn(() => 'blob:office-download'),
      revokeObjectURL: vi.fn(),
    })
    const client = testClient()
    const officeFile = { ...baseFile, path: 'report.docx', name: 'report.docx', size: 4096 }
    renderRouter(client, officeFile, textResponse({
      path: officeFile.path,
      name: officeFile.name,
      mime_type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      content: null,
      is_text: false,
      size: 4096,
      message: 'Preview unavailable.',
    }))

    expect(screen.getByText(
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    )).toBeVisible()
    expect(screen.getAllByText('report.docx')).toHaveLength(2)
    expect(screen.getByText('4 KB')).toBeVisible()
    const download = screen.getByRole('button', { name: 'Download file' })
    expect(download).toBeVisible()
    download.focus()
    await user.keyboard('{Enter}')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    expect(click).toHaveBeenCalledTimes(1)
  })

  it('does not fetch a Blob for an oversized image preview', () => {
    const createObjectURL = vi.fn()
    vi.stubGlobal('URL', {
      createObjectURL,
      revokeObjectURL: vi.fn(),
    })
    const client = testClient()
    const largeFile = { ...baseFile, path: 'large.png', name: 'large.png', size: 30 * 1024 * 1024 }
    renderRouter(client, largeFile, textResponse({
      path: largeFile.path,
      name: largeFile.name,
      mime_type: 'image/png',
      content: null,
      is_text: false,
      size: largeFile.size,
    }))

    expect(screen.getByText(/larger than the 25 MB/)).toBeVisible()
    expect(screen.getByRole('button', { name: 'Download file' })).toBeVisible()
    expect(createObjectURL).not.toHaveBeenCalled()
  })

})

describe('WorkspaceTextEditor', () => {
  beforeEach(async () => {
    useAuthStore.setState({ token: null })
    await i18n.changeLanguage('en-US')
  })

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  function renderEditor(
    file: ConversationWorkspaceFileTextResponse,
    onRefresh = vi.fn(async () => file),
  ) {
    const client = testClient()
    return {
      onRefresh,
      ...render(
        <QueryClientProvider client={client}>
          <WorkspaceTextEditor
            scope="groups"
            conversationId="group-1"
            file={file}
            onRefresh={onRefresh}
          />
        </QueryClientProvider>,
      ),
    }
  }

  it('saves dirty text with the opened version and clears the dirty state', async () => {
    const user = userEvent.setup()
    const saved = textResponse({ content: 'edited', version: 'version-2' })
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(saved), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }))
    vi.stubGlobal('fetch', fetchMock)
    renderEditor(textResponse())

    const editor = screen.getByRole('textbox', { name: 'Edit note.txt' })
    await user.clear(editor)
    await user.type(editor, 'edited')
    expect(screen.getByText('Unsaved changes')).toBeVisible()
    const save = screen.getByRole('button', { name: 'Save' })
    save.focus()
    await user.keyboard('{Enter}')

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const [, request] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(String(request.body))).toEqual({
      content: 'edited',
      version: 'version-1',
    })
    expect(await screen.findByText('Saved')).toBeVisible()
    expect(editor).toHaveValue('edited')
  })

  it('accepts IME input and leaves native editing shortcuts untouched', () => {
    renderEditor(textResponse())
    const editor = screen.getByRole('textbox', { name: 'Edit note.txt' }) as HTMLTextAreaElement

    fireEvent.compositionStart(editor)
    fireEvent.change(editor, { target: { value: '中文 AbC' } })
    fireEvent.compositionEnd(editor, { data: '中文' })

    expect(editor).toHaveValue('中文 AbC')
    for (const key of ['a', 'c', 'v', 'x', 'z', 'y']) {
      expect(fireEvent.keyDown(editor, { key, ctrlKey: true })).toBe(true)
    }
    expect(fireEvent.keyDown(editor, { key: 'CapsLock' })).toBe(true)
  })

  it('finds, replaces and saves with editor shortcuts', async () => {
    const user = userEvent.setup()
    const saved = textResponse({ content: 'updated updated world', version: 'version-2' })
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(saved), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }))
    vi.stubGlobal('fetch', fetchMock)
    renderEditor(textResponse({ content: 'hello HELLO world' }))
    const editor = screen.getByRole('textbox', { name: 'Edit note.txt' })

    fireEvent.keyDown(editor, { key: 'h', ctrlKey: true })
    const find = await screen.findByRole('searchbox', { name: 'Find' })
    await user.type(find, 'hello')
    await user.type(screen.getByRole('textbox', { name: 'Replace' }), 'updated')
    await user.click(screen.getByRole('button', { name: 'Replace all matches' }))
    expect(editor).toHaveValue('updated updated world')

    fireEvent.keyDown(editor, { key: 's', ctrlKey: true })
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    expect(JSON.parse(String((fetchMock.mock.calls[0] as [string, RequestInit])[1].body))).toEqual({
      content: 'updated updated world',
      version: 'version-1',
    })
  })

  it('scrolls the editor to the selected search result', async () => {
    const user = userEvent.setup()
    const content = `${Array.from({ length: 80 }, () => 'zzzz').join('\n')}\nneedle`
    renderEditor(textResponse({ content }))
    const editor = screen.getByRole('textbox', { name: 'Edit note.txt' }) as HTMLTextAreaElement
    Object.defineProperties(editor, {
      clientHeight: { configurable: true, value: 100 },
      clientWidth: { configurable: true, value: 400 },
    })

    fireEvent.keyDown(editor, { key: 'f', ctrlKey: true })
    await user.type(await screen.findByRole('searchbox', { name: 'Find' }), 'needle')

    await waitFor(() => expect(editor.scrollTop).toBeGreaterThan(0))
    expect(editor.selectionStart).toBe(content.indexOf('needle'))
  })

  it('keeps a newer local edit when a save response arrives late', async () => {
    const user = userEvent.setup()
    let resolveSave!: (response: Response) => void
    const saved = textResponse({ content: 'first edit', version: 'version-2' })
    const fetchMock = vi.fn(() => new Promise<Response>((resolve) => {
      resolveSave = resolve
    }))
    vi.stubGlobal('fetch', fetchMock)
    renderEditor(textResponse())

    const editor = screen.getByRole('textbox', { name: 'Edit note.txt' })
    await user.clear(editor)
    await user.type(editor, 'first edit')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    fireEvent.change(editor, { target: { value: 'newer local edit' } })

    resolveSave(new Response(JSON.stringify(saved), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }))
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Edit note.txt' })).toHaveValue('newer local edit'))
    expect(screen.getByText('Unsaved changes')).toBeVisible()
  })

  it('makes truncated text read-only and disables save', () => {
    renderEditor(textResponse({ truncated: true, content: 'partial' }))

    expect(screen.getByRole('textbox', { name: 'Edit note.txt' })).toHaveAttribute('readonly')
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
    expect(screen.getByText(/Editing and saving are disabled/)).toBeVisible()
  })

  it('localizes save and refresh failures without raw backend text', async () => {
    const user = userEvent.setup()
    await i18n.changeLanguage('zh-CN')
    const rawSaveMessage = 'workspace file is not valid UTF-8 text'
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({
      error: { code: 'invalid_input', message: rawSaveMessage },
    }), {
      status: 400,
      headers: { 'Content-Type': 'application/json' },
    })))
    const onRefresh = vi.fn().mockRejectedValue(new Error('RAW_REFRESH_BACKEND_DETAIL'))
    renderEditor(textResponse(), onRefresh)

    const editor = screen.getByRole('textbox', { name: '编辑 note.txt' })
    await user.clear(editor)
    await user.type(editor, 'edited')
    const save = screen.getByRole('button', { name: '保存' })
    save.focus()
    await user.keyboard('{Enter}')

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '保存失败：请求未通过，请检查文件或文件夹后重试。',
    )
    expect(screen.queryByText(rawSaveMessage)).not.toBeInTheDocument()

    await user.clear(editor)
    await user.type(editor, 'hello')
    const refresh = screen.getByRole('button', { name: '刷新' })
    refresh.focus()
    await user.keyboard('{Enter}')
    expect(await screen.findByText('刷新失败：无法完成此工作区操作。')).toBeVisible()
    expect(screen.queryByText('RAW_REFRESH_BACKEND_DETAIL')).not.toBeInTheDocument()
  })

  it('preserves the local buffer on 409 and refreshes only after confirmation', async () => {
    const user = userEvent.setup()
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      error: { code: 'conflict', message: 'workspace file changed since it was read' },
    }), {
      status: 409,
      headers: { 'Content-Type': 'application/json' },
    }))
    vi.stubGlobal('fetch', fetchMock)
    const diskFile = textResponse({ content: 'disk version', version: 'version-2' })
    const onRefresh = vi.fn(async () => diskFile)
    renderEditor(textResponse(), onRefresh)

    const editor = screen.getByRole('textbox', { name: 'Edit note.txt' })
    await user.clear(editor)
    await user.type(editor, 'local draft')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('The file changed on disk')).toBeVisible()
    expect(editor).toHaveValue('local draft')
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(onRefresh).not.toHaveBeenCalled()
    expect(screen.getByRole('alertdialog')).toBeVisible()
    expect(editor).toHaveValue('local draft')

    await user.click(screen.getByRole('button', { name: 'Discard and refresh' }))
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1))
    expect(editor).toHaveValue('disk version')
    expect(screen.queryByText('The file changed on disk')).toBeNull()
  })
})
