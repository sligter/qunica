import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceGitBranchSheet } from '@/components/chat/WorkspaceGitBranchSheet'
import { workspaceGitBranchesQueryKey } from '@/hooks/useWorkspaceGit'
import i18n from '@/i18n'
import type { GroupWorkspaceGitBranches } from '@/types/api'

const branches: GroupWorkspaceGitBranches = {
  branches: [
    {
      name: 'feature/RAW_原文',
      full_name: 'refs/heads/feature/RAW_原文',
      kind: 'local',
      current: false,
      upstream: null,
      ahead: 0,
      behind: 0,
    },
  ],
}

function renderSheet() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(workspaceGitBranchesQueryKey('group-1'), branches)
  return render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceGitBranchSheet
        groupId="group-1"
        open
        onOpenChange={vi.fn()}
        onError={vi.fn()}
        onSetRemote={vi.fn()}
      />
    </QueryClientProvider>,
  )
}

describe('WorkspaceGitBranchSheet i18n', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
  })
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('renders English branch controls and preserves the branch name', () => {
    renderSheet()
    expect(screen.getByRole('heading', { name: 'Branches' })).toBeVisible()
    expect(screen.getByText('Switch and manage local or remote branches.')).toBeVisible()
    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByLabelText('Rename feature/RAW_原文')).toBeVisible()
    expect(screen.getByLabelText('Delete feature/RAW_原文')).toBeVisible()
  })

  it('renders Chinese branch framing and preserves the branch name', async () => {
    await i18n.changeLanguage('zh-CN')
    renderSheet()
    expect(screen.getByRole('heading', { name: '分支' })).toBeVisible()
    expect(screen.getByText('切换并管理本地或远程分支。')).toBeVisible()
    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByLabelText('重命名 feature/RAW_原文')).toBeVisible()
    expect(screen.getByLabelText('删除 feature/RAW_原文')).toBeVisible()
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })

  it('does not switch away from a task-bound branch', () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(workspaceGitBranchesQueryKey('group-1', 'thread-1'), branches)
    render(
      <QueryClientProvider client={queryClient}>
        <WorkspaceGitBranchSheet
          groupId="group-1"
          threadId="thread-1"
          branchLocked
          open
          onOpenChange={vi.fn()}
          onError={vi.fn()}
          onSetRemote={vi.fn()}
        />
      </QueryClientProvider>,
    )

    expect(screen.getByRole('button', { name: /^feature\/RAW_原文$/ })).toBeDisabled()
  })

  it('shows localized English branch-delete failure framing with the raw Error message', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('BRANCH_DELETE_RAW_ERROR')))
    renderSheet()

    fireEvent.click(screen.getByLabelText('Delete feature/RAW_原文'))
    fireEvent.click(screen.getByRole('button', { name: 'Delete branch' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to delete branch: BRANCH_DELETE_RAW_ERROR',
    )
  })

  it('shows localized Chinese branch-delete framing with a raw non-Error rejection', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue('BRANCH_DELETE_RAW_NON_ERROR'))
    renderSheet()

    fireEvent.click(screen.getByLabelText('删除 feature/RAW_原文'))
    fireEvent.click(screen.getByRole('button', { name: '删除分支' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '删除分支失败：BRANCH_DELETE_RAW_NON_ERROR',
    )
  })
})
