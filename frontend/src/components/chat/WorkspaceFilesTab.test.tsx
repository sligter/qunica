import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceFilesTab } from '@/components/chat/WorkspaceFilesTab'
import {
  conversationWorkspaceFileListQueryKey,
  conversationWorkspaceRootsQueryKey,
} from '@/hooks/useConversationWorkspaceFiles'
import i18n from '@/i18n'
import { formatNumber } from '@/lib/format'
import { WORKSPACE_ITEM_MIME } from '@/lib/workspaceDrag'
import { useAuthStore } from '@/stores/authStore'
import type {
  ConversationScope,
  ConversationWorkspaceFileRead,
  ConversationWorkspaceRootEntry,
} from '@/types/api'

vi.mock('@/components/chat/workspace-preview/WorkspacePreviewRouter', () => ({
  WorkspacePreviewRouter: ({
    scope,
    file,
  }: {
    scope: ConversationScope
    file: ConversationWorkspaceFileRead
  }) => <div>preview:{scope}:{file.path}</div>,
}))

const rawFile: ConversationWorkspaceFileRead = {
  path: 'raw dir/README_RAW_原文.md',
  name: 'README_RAW_原文.md',
  is_dir: false,
  size: 1536,
  modified_at: '2026-07-18T00:00:00Z',
  abs_path: 'D:\\raw dir\\README_RAW_原文.md',
}

const rawFolder: ConversationWorkspaceFileRead = {
  path: 'raw dir/docs',
  name: 'docs',
  is_dir: true,
  size: null,
  modified_at: '2026-07-18T00:00:00Z',
  abs_path: 'D:\\raw dir\\docs',
}

interface RenderTabOptions {
  scope?: ConversationScope
  conversationId?: string
  workspaceId?: string | null
  files?: ConversationWorkspaceFileRead[]
  roots?: ConversationWorkspaceRootEntry[]
  agentFiles?: Record<string, ConversationWorkspaceFileRead[]>
}

function renderTab({
  scope = 'groups',
  conversationId = 'group-1',
  workspaceId = 'workspace-1',
  files = [rawFile],
  roots,
  agentFiles = {},
}: RenderTabOptions = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(
    conversationWorkspaceFileListQueryKey(scope, conversationId, ''),
    files,
  )
  if (roots) {
    queryClient.setQueryData(conversationWorkspaceRootsQueryKey(scope, conversationId), roots)
  }
  for (const [agentId, agentRootFiles] of Object.entries(agentFiles)) {
    queryClient.setQueryData(
      conversationWorkspaceFileListQueryKey(scope, conversationId, '', agentId),
      agentRootFiles,
    )
  }
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceFilesTab
        scope={scope}
        conversationId={conversationId}
        workspaceId={workspaceId}
        onInsertPaths={() => undefined}
      />
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

describe('WorkspaceFilesTab', () => {
  beforeEach(async () => {
    useAuthStore.setState({ token: null })
    await i18n.changeLanguage('en-US')
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  it('offers an agent root only when there is one, and browses it when picked', async () => {
    const user = userEvent.setup()
    const agentFile: ConversationWorkspaceFileRead = {
      path: 'draft.md',
      name: 'draft.md',
      is_dir: false,
      size: 12,
      modified_at: '2026-07-18T00:00:00Z',
      abs_path: 'D:\solo\draft.md',
    }

    // A conversation with only its own root shows no picker at all.
    const single = renderTab({
      roots: [
        {
          agent_id: null,
          display_name: null,
          workspace_mode: null,
          workspace_id: 'workspace-1',
          name: 'Shared',
          root: 'D:/shared',
          is_primary: true,
        },
      ],
    })
    expect(screen.queryByRole('combobox', { name: 'Workspace to browse' })).toBeNull()
    single.unmount()

    renderTab({
      roots: [
        {
          agent_id: null,
          display_name: null,
          workspace_mode: null,
          workspace_id: 'workspace-1',
          name: 'Shared',
          root: 'D:/shared',
          is_primary: true,
        },
        {
          agent_id: 'agent-1',
          display_name: 'Solo',
          workspace_mode: 'self',
          workspace_id: 'workspace-2',
          name: "Solo's",
          root: 'D:/solo',
          is_primary: true,
        },
      ],
      agentFiles: { 'agent-1': [agentFile] },
    })

    const picker = screen.getByRole('combobox', { name: 'Workspace to browse' })
    expect(picker).toHaveValue('')
    expect(screen.getByText('D:/shared')).toBeVisible()
    expect(screen.getByText('README_RAW_原文.md')).toBeVisible()

    await user.selectOptions(picker, 'agent-1')

    expect(screen.getByText('D:/solo')).toBeVisible()
    await waitFor(() => expect(screen.getByText('draft.md')).toBeVisible())
    expect(screen.queryByText('README_RAW_原文.md')).toBeNull()
  })

  it('keeps group mutation actions and preserves the file name', () => {
    renderTab()

    expect(screen.getByText('Workspace root')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Upload file to workspace uploads' })).toBeVisible()
    expect(screen.getByLabelText('Refresh workspace files')).toBeVisible()
    expect(screen.getByText('README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Download README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Rename README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Delete README_RAW_原文.md')).toBeVisible()
  })

  it('gives direct chats the same file operations as groups, on their own routes', async () => {
    const user = userEvent.setup()
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => (
      init?.method === 'DELETE'
        ? new Response(null, { status: 204 })
        : new Response(JSON.stringify({ ...rawFile, path: 'raw dir/renamed.md', name: 'renamed.md' }), {
            status: 200,
            headers: { 'content-type': 'application/json' },
          })
    ))
    vi.stubGlobal('fetch', fetchMock)
    const { queryClient } = renderTab({ scope: 'direct-chats', conversationId: 'chat-1' })
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')

    expect(
      screen.getByRole('button', { name: 'Upload file to workspace uploads' }),
    ).toBeVisible()
    expect(screen.getByLabelText('Rename README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Delete README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Download README_RAW_原文.md')).toBeVisible()

    await user.click(screen.getByLabelText('Rename README_RAW_原文.md'))
    await user.clear(screen.getByLabelText('Rename path'))
    await user.type(screen.getByLabelText('Rename path'), 'raw dir/renamed.md')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining(
        '/direct-chats/chat-1/workspace-files/rename?path=raw%20dir%2FREADME_RAW_',
      ),
      expect.objectContaining({
        method: 'PATCH',
        body: JSON.stringify({ new_path: 'raw dir/renamed.md' }),
      }),
    ))
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'workspace-files'],
    })

    await user.click(screen.getByLabelText('Delete README_RAW_原文.md'))
    await user.click(screen.getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining(
        '/direct-chats/chat-1/workspace-files?path=raw%20dir%2FREADME_RAW_',
      ),
      expect.objectContaining({ method: 'DELETE' }),
    ))
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'workspace-files'],
    })
  })

  it('emits structured drag items with accessible file and directory state', () => {
    renderTab({ files: [rawFolder, rawFile] })
    const fileButton = screen.getByText('README_RAW_原文.md').closest('button')!
    const fileRow = fileButton.closest('li')
    const folderButton = screen.getByText('docs').closest('button')!
    const folderRow = folderButton.closest('li')
    expect(fileRow).not.toBeNull()
    expect(folderRow).not.toBeNull()
    expect(fileButton).toHaveAccessibleDescription(
      'Drag this file to the composer to attach it.',
    )
    expect(folderButton).toHaveAccessibleDescription(
      'Drag this folder to the composer to insert its relative path.',
    )
    expect(fileButton).toHaveAttribute('draggable', 'true')
    expect(fileButton).toHaveAttribute('aria-grabbed', 'false')
    expect(folderButton).toHaveAttribute('aria-grabbed', 'false')

    const setData = vi.fn()
    fireEvent.dragStart(fileButton, {
      dataTransfer: { effectAllowed: 'none', setData },
    })

    const structuredCall = setData.mock.calls.find(([type]) => type === WORKSPACE_ITEM_MIME)
    expect(structuredCall).toBeDefined()
    expect(JSON.parse(String(structuredCall?.[1]))).toEqual([
      {
        version: 1,
        path: rawFile.path,
        name: rawFile.name,
        kind: 'file',
      },
    ])
    expect(setData).toHaveBeenCalledWith('text/plain', rawFile.path)
    expect(fileButton).toHaveAttribute('aria-grabbed', 'true')
    fireEvent.dragEnd(fileButton)
    expect(fileButton).toHaveAttribute('aria-grabbed', 'false')

    setData.mockClear()
    fireEvent.dragStart(screen.getByLabelText('Download README_RAW_原文.md'), {
      dataTransfer: { effectAllowed: 'none', setData },
    })
    expect(setData).not.toHaveBeenCalled()
  })

  it('preserves keyboard opening and provides keyboard context-menu navigation', async () => {
    const user = userEvent.setup()
    renderTab()
    const fileButton = screen.getByText('README_RAW_原文.md').closest('button')!
    fileButton.focus()
    await user.keyboard('{Enter}')
    expect(screen.getByRole('dialog')).toBeVisible()

    await user.keyboard('{Escape}')
    fireEvent.keyDown(fileButton, { key: 'F10', shiftKey: true })
    expect(screen.getByRole('menu', { name: 'File actions' })).toBeVisible()
    expect(screen.getByRole('menuitem', { name: 'Open preview' })).toHaveFocus()

    await user.keyboard('{ArrowDown}')
    expect(screen.getByRole('menuitem', { name: 'Download' })).toHaveFocus()

    await user.keyboard('{Escape}')
    expect(screen.queryByRole('menu', { name: 'File actions' })).not.toBeInTheDocument()
    await waitFor(() => expect(fileButton).toHaveFocus())
  })

  it('localizes the Chinese preview framing without changing the raw path', async () => {
    await i18n.changeLanguage('zh-CN')
    renderTab()

    fireEvent.click(screen.getByText('README_RAW_原文.md'))
    expect(screen.getByRole('dialog')).toBeVisible()
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
    expect(screen.getByRole('heading', { name: 'raw dir/README_RAW_原文.md' })).toBeVisible()
    expect(screen.getByText('预览由服务器限制，大文件可能会被截断。')).toBeVisible()
    expect(screen.getByText('preview:groups:raw dir/README_RAW_原文.md')).toBeVisible()
  })

  it('shows localized English delete failure framing without raw diagnostics', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('DELETE_RAW_ERROR')))
    renderTab()

    fireEvent.click(screen.getByLabelText('Delete README_RAW_原文.md'))
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to delete path: The workspace operation could not be completed.',
    )
    expect(screen.queryByText('DELETE_RAW_ERROR')).not.toBeInTheDocument()
  })

  it('shows localized Chinese delete failure framing without raw non-Error text', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue('DELETE_RAW_NON_ERROR'))
    renderTab()

    fireEvent.click(screen.getByLabelText('删除 README_RAW_原文.md'))
    fireEvent.click(screen.getByRole('button', { name: '删除' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '删除路径失败：无法完成此工作区操作。',
    )
    expect(screen.queryByText('DELETE_RAW_NON_ERROR')).not.toBeInTheDocument()
  })

  it('uses the locale helper for selected-count display while keeping numeric plural input', async () => {
    const count = 12345

    expect(
      i18n.t('common:workspaceOperations.selectedCount', {
        count,
        formattedCount: formatNumber(count, 'en-US'),
      }),
    ).toBe('12,345 selected')

    await i18n.changeLanguage('zh-CN')
    expect(
      i18n.t('common:workspaceOperations.selectedCount', {
        count,
        formattedCount: formatNumber(count, 'zh-CN'),
      }),
    ).toBe('已选 12,345 项')
  })
})
