import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
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
  afterEach(cleanup)

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
  })
})
