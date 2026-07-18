import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceFilesTab } from '@/components/chat/WorkspaceFilesTab'
import { workspaceFilesQueryKey } from '@/hooks/useGroupFiles'
import i18n from '@/i18n'
import { formatNumber } from '@/lib/format'
import type { GroupWorkspaceFilePreview, GroupWorkspaceFileRead } from '@/types/api'

const rawFile: GroupWorkspaceFileRead = {
  path: 'raw dir/README_RAW_原文.md',
  name: 'README_RAW_原文.md',
  is_dir: false,
  size: 1536,
  modified_at: '2026-07-18T00:00:00Z',
  abs_path: 'D:\\raw dir\\README_RAW_原文.md',
}

const rawPreview: GroupWorkspaceFilePreview = {
  path: rawFile.path,
  name: rawFile.name,
  is_text: true,
  content: 'CONTENT_RAW_原文\nline 2',
  truncated: false,
  message: null,
  size: rawFile.size,
}

function renderTab() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(workspaceFilesQueryKey('group-1'), [rawFile])
  queryClient.setQueryData(
    ['groups', 'group-1', 'workspace-files', 'preview', rawFile.path],
    rawPreview,
  )
  return render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceFilesTab groupId="group-1" onInsertPaths={() => undefined} />
    </QueryClientProvider>,
  )
}

describe('WorkspaceFilesTab i18n', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
  })
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('renders English file actions while preserving the file name', () => {
    renderTab()

    expect(screen.getByText('Workspace root')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Upload file to workspace uploads' })).toBeVisible()
    expect(screen.getByLabelText('Refresh workspace files')).toBeVisible()
    expect(screen.getByText('README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Download README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Rename README_RAW_原文.md')).toBeVisible()
    expect(screen.getByLabelText('Delete README_RAW_原文.md')).toBeVisible()
  })

  it('localizes the Chinese preview framing without changing file content', async () => {
    await i18n.changeLanguage('zh-CN')
    renderTab()

    fireEvent.click(screen.getByText('README_RAW_原文.md'))
    expect(screen.getByRole('dialog')).toBeVisible()
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
    expect(screen.getByRole('heading', { name: 'raw dir/README_RAW_原文.md' })).toBeVisible()
    expect(screen.getByText('预览由服务器限制，大文件可能会被截断。')).toBeVisible()
    expect(document.querySelector('pre')?.textContent).toBe('CONTENT_RAW_原文\nline 2')
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
