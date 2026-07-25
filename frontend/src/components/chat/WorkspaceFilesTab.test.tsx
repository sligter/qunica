import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceFilesTab } from '@/components/chat/WorkspaceFilesTab'
import { conversationWorkspaceFileListQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import i18n from '@/i18n'
import { formatNumber } from '@/lib/format'
import { WORKSPACE_ITEM_MIME } from '@/lib/workspaceDrag'
import { useAuthStore } from '@/stores/authStore'
import type { ConversationScope, ConversationWorkspaceFileRead } from '@/types/api'

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
}

function renderTab({
  scope = 'groups',
  conversationId = 'group-1',
  workspaceId = 'workspace-1',
  files = [rawFile],
}: RenderTabOptions = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(
    conversationWorkspaceFileListQueryKey(scope, conversationId, ''),
    files,
  )
  return render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceFilesTab
        scope={scope}
        conversationId={conversationId}
        workspaceId={workspaceId}
        onInsertPaths={() => undefined}
      />
    </QueryClientProvider>,
  )
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

  it('keeps direct-chat files read-only while retaining download and preview', async () => {
    const user = userEvent.setup()
    renderTab({ scope: 'direct-chats', conversationId: 'chat-1' })

    expect(screen.queryByRole('button', { name: 'Upload file to workspace uploads' })).toBeNull()
    expect(screen.queryByLabelText('Rename README_RAW_原文.md')).toBeNull()
    expect(screen.queryByLabelText('Delete README_RAW_原文.md')).toBeNull()
    expect(screen.getByLabelText('Download README_RAW_原文.md')).toBeVisible()

    await user.click(screen.getByText('README_RAW_原文.md'))
    expect(screen.getByRole('dialog')).toBeVisible()
    expect(screen.getByText('preview:direct-chats:raw dir/README_RAW_原文.md')).toBeVisible()
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
    expect(fileRow).toHaveAttribute('aria-grabbed', 'false')
    expect(folderRow).toHaveAttribute('aria-grabbed', 'false')

    const setData = vi.fn()
    fireEvent.dragStart(fileRow!, {
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
    expect(fileRow).toHaveAttribute('aria-grabbed', 'true')
    fireEvent.dragEnd(fileRow!)
    expect(fileRow).toHaveAttribute('aria-grabbed', 'false')
  })

  it('preserves keyboard opening and the context menu action', async () => {
    const user = userEvent.setup()
    renderTab()
    const fileButton = screen.getByText('README_RAW_原文.md').closest('button')!
    fileButton.focus()
    await user.keyboard('{Enter}')
    expect(screen.getByRole('dialog')).toBeVisible()

    await user.keyboard('{Escape}')
    fireEvent.contextMenu(fileButton.closest('li')!)
    expect(screen.getByRole('menu', { name: 'File actions' })).toBeVisible()
    expect(screen.getByRole('menuitem', { name: 'Open preview' })).toBeVisible()
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

  it('shows localized English delete failure framing with the raw Error message', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('DELETE_RAW_ERROR')))
    renderTab()

    fireEvent.click(screen.getByLabelText('Delete README_RAW_原文.md'))
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to delete path: DELETE_RAW_ERROR',
    )
  })

  it('shows localized Chinese delete failure framing with a raw non-Error rejection', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue('DELETE_RAW_NON_ERROR'))
    renderTab()

    fireEvent.click(screen.getByLabelText('删除 README_RAW_原文.md'))
    fireEvent.click(screen.getByRole('button', { name: '删除' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '删除路径失败：DELETE_RAW_NON_ERROR',
    )
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
