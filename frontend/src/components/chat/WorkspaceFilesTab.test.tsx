import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { WorkspaceFilesTab } from '@/components/chat/WorkspaceFilesTab'
import { workspaceFilesQueryKey } from '@/hooks/useGroupFiles'
import i18n from '@/i18n'
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
  afterEach(cleanup)

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
    expect(screen.getByRole('heading', { name: 'raw dir/README_RAW_原文.md' })).toBeVisible()
    expect(screen.getByText('预览由服务器限制，大文件可能会被截断。')).toBeVisible()
    expect(document.querySelector('pre')?.textContent).toBe('CONTENT_RAW_原文\nline 2')
  })
})
