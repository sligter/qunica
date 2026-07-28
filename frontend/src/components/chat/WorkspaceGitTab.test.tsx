import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceGitTab } from '@/components/chat/WorkspaceGitTab'
import { workspaceGitDiffQueryKey, workspaceGitQueryKey } from '@/hooks/useWorkspaceGit'
import i18n from '@/i18n'
import type { GroupWorkspaceGitDiff, GroupWorkspaceGitStatus } from '@/types/api'

const rawPath = 'src/RAW_原文.ts'
const rawPatch = [
  'diff --git a/src/RAW_原文.ts b/src/RAW_原文.ts',
  '--- a/src/RAW_原文.ts',
  '+++ b/src/RAW_原文.ts',
  '@@ -1 +1 @@',
  '-OLD_RAW_DIFF_原文',
  '+NEW_RAW_DIFF_原文',
].join('\n')

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

function renderTab(nextStatus: GroupWorkspaceGitStatus = status) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(workspaceGitQueryKey('group-1'), nextStatus)
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
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('renders English Git framing while preserving branch, path, and diff data', () => {
    const { container } = renderTab()

    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByText('12,345 ahead, 1 behind')).toBeVisible()
    expect(screen.getByRole('button', { name: `Unstage ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: `Stage ${rawPath}` })).toBeVisible()
    expect(screen.getByText('origin_RAW / 1 staged')).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Stash worktree changes' })).not.toBeInTheDocument()
    expect(container.querySelector('pre')).toHaveTextContent('+NEW_RAW_DIFF_原文')
  })

  it('enables the commit message as soon as staging returns the updated status', async () => {
    const unstagedStatus: GroupWorkspaceGitStatus = {
      ...status,
      dirty_counts: { ...status.dirty_counts, staged: 0 },
      files: status.files.map((file) => ({ ...file, staged: false })),
    }
    const stagedStatus: GroupWorkspaceGitStatus = {
      ...status,
      files: status.files.map((file) => ({ ...file, unstaged: false })),
    }
    const fetchMock = vi.fn().mockResolvedValue(
        new Response(JSON.stringify(stagedStatus), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const { container } = renderTab(unstagedStatus)

    const commitMessage = screen.getByRole('textbox', { name: 'Commit message' })
    expect(commitMessage).toBeDisabled()
    fireEvent.click(within(container.querySelector('header')!).getByRole('button', { name: 'Stage all' }))

    await waitFor(() => expect(commitMessage).toBeEnabled())
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/workspace-git/stage'),
      expect.objectContaining({ body: '{"paths":[]}' }),
    )
  })

  it('highlights patch lines and expands the diff across the workspace panel', () => {
    const { container } = renderTab()

    expect(container.querySelector('[data-diff-line="addition"]')).toHaveTextContent('+NEW_RAW_DIFF')
    expect(container.querySelector('[data-diff-line="deletion"]')).toHaveTextContent('-OLD_RAW_DIFF')
    expect(container.querySelector('[data-diff-line="hunk"]')).toHaveTextContent('@@ -1 +1 @@')
    expect(container.querySelector('[data-diff-line="meta"]')).toHaveTextContent('diff --git')

    const toggle = screen.getByRole('button', { name: 'Expand diff' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(toggle)

    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(toggle).toHaveAccessibleName('Collapse diff')
    expect(screen.queryByRole('button', { name: `Stage ${rawPath}` })).not.toBeInTheDocument()
  })

  it('renders Chinese Git framing while preserving branch, path, and diff data', async () => {
    await i18n.changeLanguage('zh-CN')
    const { container } = renderTab()

    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByText('领先 12,345，落后 1')).toBeVisible()
    expect(screen.getByRole('button', { name: `取消暂存 ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: `暂存 ${rawPath}` })).toBeVisible()
    expect(screen.getByText('origin_RAW / 1 项暂存')).toBeVisible()
    expect(container.querySelector('pre')).toHaveTextContent('+NEW_RAW_DIFF_原文')
  })

  it('frames an unknown repository state while preserving the raw wire value', async () => {
    await i18n.changeLanguage('zh-CN')
    renderTab({
      ...status,
      state: 'future_repo_state' as GroupWorkspaceGitStatus['state'],
    })

    expect(screen.getByText('仓库状态：future_repo_state')).toBeVisible()
  })

  it('shows localized Chinese discard failure framing for a raw non-Error rejection', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue('DISCARD_RAW_NON_ERROR'))
    renderTab()

    fireEvent.click(screen.getByRole('button', { name: `丢弃 ${rawPath}` }))
    fireEvent.click(screen.getByRole('button', { name: '丢弃' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '丢弃 Git 更改失败：DISCARD_RAW_NON_ERROR',
    )
  })

  it('shows localized English discard failure framing with the raw Error message', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('DISCARD_RAW_ERROR')))
    renderTab()

    fireEvent.click(screen.getByRole('button', { name: `Discard ${rawPath}` }))
    fireEvent.click(screen.getByRole('button', { name: 'Discard' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to discard Git changes: DISCARD_RAW_ERROR',
    )
  })

  it('localizes the branch sheet and remote dialog close buttons in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    renderTab()

    fireEvent.click(screen.getByTitle('管理分支'))
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: '远程 URL' }))
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })
})
