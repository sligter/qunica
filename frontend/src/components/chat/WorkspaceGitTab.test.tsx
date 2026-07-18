import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { WorkspaceGitTab } from '@/components/chat/WorkspaceGitTab'
import { workspaceGitDiffQueryKey, workspaceGitQueryKey } from '@/hooks/useWorkspaceGit'
import i18n from '@/i18n'
import type { GroupWorkspaceGitDiff, GroupWorkspaceGitStatus } from '@/types/api'

const rawPath = 'src/RAW_原文.ts'
const rawPatch = 'diff --git a/src/RAW_原文.ts b/src/RAW_原文.ts\n+RAW_DIFF_原文'

const status: GroupWorkspaceGitStatus = {
  available: true,
  status: 'ready',
  branch: 'feature/RAW_原文',
  upstream: 'origin/feature/RAW_原文',
  remote_name: 'origin_RAW',
  remote_url: 'https://example.invalid/raw.git',
  ahead: 12345,
  behind: 1,
  stash_count: 2,
  clean: false,
  dirty_counts: { staged: 1, unstaged: 1, untracked: 0, conflicted: 0 },
  files: [
    {
      path: rawPath,
      old_path: null,
      status: 'M',
      staged: true,
      unstaged: true,
      untracked: false,
      conflicted: false,
    },
  ],
  message: null,
  state: null,
}

const diff: GroupWorkspaceGitDiff = {
  mode: 'worktree',
  base_ref: null,
  head_ref: null,
  path: null,
  patch: rawPatch,
  stat: 'RAW_STAT',
  truncated: false,
  binary_files: [],
}

function renderTab() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(workspaceGitQueryKey('group-1'), status)
  queryClient.setQueryData(workspaceGitDiffQueryKey('group-1', 'worktree'), diff)
  return render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceGitTab groupId="group-1" />
    </QueryClientProvider>,
  )
}

describe('WorkspaceGitTab i18n', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
  })
  afterEach(cleanup)

  it('renders English Git framing while preserving branch, path, and diff data', () => {
    const { container } = renderTab()

    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByText('12,345 ahead, 1 behind')).toBeVisible()
    expect(screen.getByRole('button', { name: `Unstage ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: `Stage ${rawPath}` })).toBeVisible()
    expect(container.querySelector('pre')?.textContent).toBe(rawPatch)
  })

  it('renders Chinese Git framing while preserving branch, path, and diff data', async () => {
    await i18n.changeLanguage('zh-CN')
    const { container } = renderTab()

    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByText('领先 12,345，落后 1')).toBeVisible()
    expect(screen.getByRole('button', { name: `取消暂存 ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: `暂存 ${rawPath}` })).toBeVisible()
    expect(container.querySelector('pre')?.textContent).toBe(rawPatch)
  })
})
