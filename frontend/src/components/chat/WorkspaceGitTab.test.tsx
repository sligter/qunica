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
  queryClient.setQueryData(workspaceGitDiffQueryKey('group-1', 'branch'), {
    ...diff,
    mode: 'branch',
    base_ref: nextStatus.upstream,
    head_ref: 'HEAD',
  })
  queryClient.setQueryData(workspaceGitDiffQueryKey('group-1', 'branch', rawPath), {
    ...diff,
    mode: 'branch',
    base_ref: nextStatus.upstream,
    head_ref: 'HEAD',
    path: rawPath,
  })
  queryClient.setQueryData(workspaceGitDiffQueryKey('group-1', 'worktree', rawPath), {
    ...diff,
    path: rawPath,
  })
  queryClient.setQueryData(workspaceGitDiffQueryKey('group-1', 'staged', rawPath), {
    ...diff,
    mode: 'staged',
    path: rawPath,
  })
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

  it('renders the compact Orca-style Git controls without loading a diff eagerly', () => {
    const { container } = renderTab()

    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByText('12,345 ahead, 1 behind')).toBeVisible()
    expect(screen.getByText(status.upstream!)).toBeVisible()
    expect(screen.getByText('Committed changes')).toBeVisible()
    expect(screen.getByText('Staged changes')).toBeVisible()
    expect(screen.getByText('Changes')).toBeVisible()
    expect(screen.getByRole('button', { name: `Unstage ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: `Stage ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: 'Create PR' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'More Git actions' }))
    const menu = screen.getByRole('menu', { name: 'Git actions' })
    expect(within(menu).getByRole('menuitem', { name: 'Commit & Push' })).toBeVisible()
    expect(within(menu).getByRole('menuitem', { name: /^Force Push/ })).toBeVisible()
    expect(within(menu).getByRole('menuitem', { name: /^Sync/ })).toBeVisible()
    expect(within(menu).getByRole('menuitem', { name: /^Rebase from/ })).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Stash worktree changes' })).not.toBeInTheDocument()
    expect(container.querySelector('pre')).not.toBeInTheDocument()

    const committed = screen.getByRole('button', { name: /^Committed changes/ })
    expect(committed).toHaveAttribute('aria-expanded', 'true')
    fireEvent.click(committed)
    expect(committed).toHaveAttribute('aria-expanded', 'false')
  })

  it('uses Push as the primary action for a clean branch with outgoing commits', () => {
    const { container } = renderTab({
      ...status,
      clean: true,
      dirty_counts: { staged: 0, unstaged: 0, untracked: 0, conflicted: 0 },
      files: [],
    })

    expect(within(container.querySelector('header')!).getByRole('button', { name: 'Push' })).toBeEnabled()
    expect(screen.getByText('Committed changes')).toBeVisible()
  })

  it('syncs by pulling before pushing', async () => {
    const cleanStatus: GroupWorkspaceGitStatus = {
      ...status,
      ahead: 0,
      behind: 0,
      clean: true,
      dirty_counts: { staged: 0, unstaged: 0, untracked: 0, conflicted: 0 },
      files: [],
    }
    const fetchMock = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = String(input)
      const body = url.includes('/workspace-git/log?')
        ? { commits: [], has_more: false }
        : cleanStatus
      return Promise.resolve(new Response(JSON.stringify(body), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }))
    })
    vi.stubGlobal('fetch', fetchMock)
    const { container } = renderTab(cleanStatus)

    fireEvent.click(within(container.querySelector('header')!).getByRole('button', { name: 'Sync' }))

    await waitFor(() => {
      const operations = fetchMock.mock.calls
        .map(([input]) => String(input))
        .filter((url) => /workspace-git\/(pull|push)$/.test(url))
      expect(operations).toEqual([
        expect.stringContaining('/workspace-git/pull'),
        expect.stringContaining('/workspace-git/push'),
      ])
    })
  })

  it('opens the provider-native pull request page', () => {
    const open = vi.spyOn(window, 'open').mockImplementation(() => null)
    renderTab({
      ...status,
      branch: 'feature/git ui',
      upstream: 'origin/feature/git ui',
      remote_url: 'git@github.com:openai/codex.git',
    })

    fireEvent.click(screen.getByText('Create PR').closest('button')!)

    expect(open).toHaveBeenCalledWith(
      'https://github.com/openai/codex/pull/new/feature/git%20ui',
      '_blank',
      'noopener,noreferrer',
    )
  })

  it('enables the primary commit action after staging and entering a message', async () => {
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
    expect(commitMessage).toBeEnabled()
    expect(within(container.querySelector('header')!).queryByRole('button', { name: 'Commit' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Stage all' }))

    const commit = await screen.findByRole('button', { name: 'Commit' })
    expect(commit).toBeDisabled()
    fireEvent.change(commitMessage, { target: { value: 'ship the Git panel' } })
    expect(commit).toBeEnabled()
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/workspace-git/stage'),
      expect.objectContaining({ body: '{"paths":[]}' }),
    )
  })

  it('loads and highlights a file diff only after the file is selected', async () => {
    const { container } = renderTab()

    expect(container.querySelector('[data-diff-line="addition"]')).not.toBeInTheDocument()
    fireEvent.click(screen.getAllByTitle(rawPath)[0])

    expect(await screen.findByRole('dialog')).toBeVisible()
    expect(document.querySelector('[data-diff-line="addition"]')).toHaveTextContent('+NEW_RAW_DIFF')
    expect(document.querySelector('[data-diff-line="deletion"]')).toHaveTextContent('-OLD_RAW_DIFF')
    expect(document.querySelector('[data-diff-line="hunk"]')).toHaveTextContent('@@ -1 +1 @@')
    expect(document.querySelector('[data-diff-line="meta"]')).toHaveTextContent('diff --git')
  })

  it('renders the compact Git controls in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    const { container } = renderTab()

    expect(screen.getByText('feature/RAW_原文')).toBeVisible()
    expect(screen.getByText('领先 12,345，落后 1')).toBeVisible()
    expect(screen.getByRole('button', { name: '创建 PR' })).toBeDisabled()
    expect(screen.getByText('已提交的更改')).toBeVisible()
    expect(screen.getByText('已暂存的更改')).toBeVisible()
    expect(screen.getByText('更改')).toBeVisible()
    expect(screen.getByRole('button', { name: `取消暂存 ${rawPath}` })).toBeVisible()
    expect(screen.getByRole('button', { name: `暂存 ${rawPath}` })).toBeVisible()
    expect(container.querySelector('pre')).not.toBeInTheDocument()
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
