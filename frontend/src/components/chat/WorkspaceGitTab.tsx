import { useEffect, useRef, useState, type ReactNode } from 'react'
import {
  ArrowDown,
  ArrowRight,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  FileCode2,
  GitBranch,
  GitCommitHorizontal,
  GitPullRequest,
  LoaderCircle,
  Minus,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'

import { WorkspaceGitBranchSheet } from '@/components/chat/WorkspaceGitBranchSheet'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  useCommitGroupWorkspaceGit,
  useCreateGroupWorkspaceGitBranchFromCommit,
  useDiscardGroupWorkspaceGit,
  useFetchGroupWorkspaceGit,
  useForcePushGroupWorkspaceGit,
  useGenerateGroupWorkspaceGitCommitMessage,
  useGroupWorkspaceGitCommit,
  useGroupWorkspaceGitCommitDiff,
  useGroupWorkspaceGitDiff,
  useGroupWorkspaceGitLog,
  useGroupWorkspaceGitStatus,
  useIgnoreGroupWorkspaceGit,
  useInitGroupWorkspaceGit,
  usePullGroupWorkspaceGit,
  usePushGroupWorkspaceGit,
  useRebaseGroupWorkspaceGit,
  useSetGroupWorkspaceGitRemote,
  useStageGroupWorkspaceGit,
  useUnstageGroupWorkspaceGit,
} from '@/hooks/useWorkspaceGit'
import { normalizeLanguage } from '@/i18n'
import { ApiError } from '@/lib/api-v2/client'
import { formatNumber } from '@/lib/format'
import { isDesktopRuntime } from '@/lib/runtime'
import { cn } from '@/lib/utils'
import type {
  ConversationScope,
  GroupWorkspaceGitCommitSummary,
  GroupWorkspaceGitFileStatus,
  GroupWorkspaceGitStatus,
} from '@/types/api'

interface WorkspaceGitTabProps {
  groupId: string | undefined
  scope?: ConversationScope
}

type ReviewMode = 'changes' | 'history'
type ChangeSelection = { path: string; mode: 'worktree' | 'staged' | 'branch' } | null
type RemoteOperation = (() => Promise<unknown>) | null
type RepositoryState = NonNullable<GroupWorkspaceGitStatus['state']>
type DiffLineKind = 'addition' | 'deletion' | 'hunk' | 'meta' | 'context'

const COMMIT_PROMPT_STORAGE_KEY = 'ag-swarmer:git:commit-message-prompt'

function readCommitPrompt(): string {
  try {
    return localStorage.getItem(COMMIT_PROMPT_STORAGE_KEY) ?? ''
  } catch {
    return ''
  }
}

const repositoryStateKeys = {
  conflict: 'workspace.gitPanel.conflicts',
  detached: 'workspace.gitPanel.detached',
  initial: 'workspace.gitPanel.initial',
} as const satisfies Record<RepositoryState, string>

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function isMissingRemote(error: unknown) {
  return error instanceof ApiError && error.code === 'missing_remote'
}

function isRepositoryState(value: string): value is RepositoryState {
  return Object.prototype.hasOwnProperty.call(repositoryStateKeys, value)
}

function statusSummary(
  status: ReturnType<typeof useGroupWorkspaceGitStatus>['data'],
  t: TFunction<'chat'>,
  language: 'en-US' | 'zh-CN',
) {
  if (!status) return t('workspace.gitPanel.workspaceGit')
  const repositoryState = status.state as string | null | undefined
  if (repositoryState) {
    return isRepositoryState(repositoryState)
      ? t(repositoryStateKeys[repositoryState])
      : t('common:wireLabels.unknownRepositoryState', { value: repositoryState })
  }
  const ahead = status.ahead ? formatNumber(status.ahead, language) : null
  const behind = status.behind ? formatNumber(status.behind, language) : null
  if (ahead && behind) return t('workspace.gitPanel.aheadBehind', { ahead, behind })
  if (ahead) return t('workspace.gitPanel.ahead', { count: ahead })
  if (behind) return t('workspace.gitPanel.behind', { count: behind })
  return status.clean
    ? t('workspace.gitPanel.clean')
    : t('workspace.gitPanel.changed', {
        count: status.files.length,
        formattedCount: formatNumber(status.files.length, language),
      })
}

function diffLineKind(line: string): DiffLineKind {
  if (line.startsWith('@@')) return 'hunk'
  if (line.startsWith('+') && !line.startsWith('+++')) return 'addition'
  if (line.startsWith('-') && !line.startsWith('---')) return 'deletion'
  if (
    line.startsWith('diff --git')
    || line.startsWith('index ')
    || line.startsWith('---')
    || line.startsWith('+++')
  ) return 'meta'
  return 'context'
}

const diffLineClassNames: Record<DiffLineKind, string> = {
  addition: 'border-l-2 border-emerald-500 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
  deletion: 'border-l-2 border-red-500 bg-red-500/10 text-red-700 dark:text-red-300',
  hunk: 'border-l-2 border-sky-500 bg-sky-500/10 text-sky-700 dark:text-sky-300',
  meta: 'bg-muted/50 text-muted-foreground',
  context: '',
}

function DiffPatch({ content, highlight }: { content: string; highlight: boolean }) {
  return (
    <pre className="min-h-0 flex-1 overflow-auto bg-muted/15 py-3 font-mono text-2xs leading-5">
      <code className="block w-max min-w-full">
        {content.split('\n').map((line, index) => {
          const kind = highlight ? diffLineKind(line) : 'context'
          return (
            <span
              key={index}
              data-diff-line={kind}
              className={cn('block min-h-5 w-full px-3', diffLineClassNames[kind])}
            >
              {line || ' '}
            </span>
          )
        })}
      </code>
    </pre>
  )
}

function splitPath(path: string) {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return slash === -1
    ? { directory: '', name: path }
    : { directory: path.slice(0, slash), name: path.slice(slash + 1) }
}

function statusTone(file: GroupWorkspaceGitFileStatus) {
  if (file.conflicted) return 'text-red-500'
  if (file.untracked || file.status.includes('A')) return 'text-emerald-500'
  if (file.status.includes('D')) return 'text-red-500'
  return 'text-amber-500'
}

function committedFiles(patch: string): GroupWorkspaceGitFileStatus[] {
  const files: GroupWorkspaceGitFileStatus[] = []
  let current: GroupWorkspaceGitFileStatus | undefined
  for (const line of patch.split('\n')) {
    if (line.startsWith('diff --git a/')) {
      const paths = line.slice('diff --git a/'.length)
      const separator = paths.lastIndexOf(' b/')
      if (separator < 0) continue
      const oldPath = paths.slice(0, separator)
      const path = paths.slice(separator + 3)
      current = {
        path,
        old_path: oldPath === path ? null : oldPath,
        status: oldPath === path ? 'M' : 'R',
        staged: false,
        unstaged: false,
        untracked: false,
        conflicted: false,
      }
      files.push(current)
      continue
    }
    if (line.startsWith('new file mode ') && current) current.status = 'A'
    if (line.startsWith('deleted file mode ') && current) current.status = 'D'
  }
  return files
}

function CollapsibleHeader({
  action,
  count,
  expanded,
  onToggle,
  title,
}: {
  action?: ReactNode
  count?: number
  expanded: boolean
  onToggle: () => void
  title: string
}) {
  return (
    <header className="flex h-8 items-center gap-1 px-2">
      <button
        type="button"
        className="flex min-w-0 flex-1 items-center gap-1 rounded px-1 py-0.5 text-left text-xs font-semibold hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        aria-expanded={expanded}
        onClick={onToggle}
      >
        <ChevronDown className={cn('h-3.5 w-3.5 shrink-0 transition-transform', !expanded && '-rotate-90')} />
        <span className="truncate">{title}</span>
        {count === undefined ? null : (
          <span className="font-mono text-[10px] text-muted-foreground">{count}</span>
        )}
      </button>
      {action}
    </header>
  )
}

function ChangeSection({
  action,
  disabled,
  files,
  onAction,
  onDiscard,
  onIgnore,
  onSelect,
  selection,
  title,
}: {
  action: 'stage' | 'unstage' | null
  disabled: boolean
  files: GroupWorkspaceGitFileStatus[]
  onAction?: (paths: string[]) => void
  onDiscard?: (file: GroupWorkspaceGitFileStatus) => void
  onIgnore?: (file: GroupWorkspaceGitFileStatus) => void
  onSelect: (selection: NonNullable<ChangeSelection>) => void
  selection: ChangeSelection
  title: string
}) {
  const { t } = useTranslation('chat')
  const [expanded, setExpanded] = useState(true)
  if (files.length === 0) return null
  const diffMode = action === 'stage' ? 'worktree' : action === 'unstage' ? 'staged' : 'branch'
  const allLabel = action === 'stage'
    ? t('workspace.gitPanel.stageAll')
    : t('workspace.gitPanel.unstageAll')

  return (
    <section className="border-b border-border/70">
      <CollapsibleHeader
        title={title}
        count={files.length}
        expanded={expanded}
        onToggle={() => setExpanded((value) => !value)}
        action={action ? (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            disabled={disabled}
            onClick={() => onAction?.([])}
            aria-label={allLabel}
            title={allLabel}
          >
            {action === 'stage' ? <Plus className="h-3.5 w-3.5" /> : <Minus className="h-3.5 w-3.5" />}
          </Button>
        ) : undefined}
      />
      {expanded ? (
        <ul className="pb-1">
          {files.map((file) => {
            const path = splitPath(file.path)
            const selected = selection?.path === file.path && selection.mode === diffMode
            return (
              <li
                key={`${title}:${file.path}`}
                className={cn('group flex min-w-0 items-center px-2 hover:bg-muted/70', selected && 'bg-muted')}
              >
                <button
                  type="button"
                  className="flex h-7 min-w-0 flex-1 items-center gap-2 text-left"
                  title={file.path}
                  onClick={() => onSelect({ path: file.path, mode: diffMode })}
                >
                  <FileCode2 className={cn('h-4 w-4 shrink-0', statusTone(file))} />
                  <span className="min-w-0 flex-1 truncate text-xs">
                    <span className="font-medium">{path.name}</span>
                    {path.directory ? <span className="ml-1 text-[10px] text-muted-foreground">{path.directory}</span> : null}
                  </span>
                  <span className={cn('shrink-0 font-mono text-[10px] font-semibold', statusTone(file))}>
                    {file.status.trim() || 'M'}
                  </span>
                </button>
                {action ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                    disabled={disabled}
                    onClick={() => onAction?.([file.path])}
                    aria-label={action === 'stage' ? t('workspace.gitPanel.stageNamed', { path: file.path }) : t('workspace.gitPanel.unstageNamed', { path: file.path })}
                    title={action === 'stage' ? t('workspace.stage') : t('workspace.unstage')}
                  >
                    {action === 'stage' ? <Plus className="h-3.5 w-3.5" /> : <Minus className="h-3.5 w-3.5" />}
                  </Button>
                ) : null}
                {onDiscard ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                    disabled={disabled}
                    onClick={() => onDiscard(file)}
                    aria-label={t('workspace.gitPanel.discardNamed', { path: file.path })}
                    title={t('workspace.gitPanel.discardChanges')}
                  >
                    <Trash2 className="h-3.5 w-3.5 text-destructive" />
                  </Button>
                ) : null}
                {onIgnore && file.untracked ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                    disabled={disabled}
                    onClick={() => onIgnore(file)}
                    aria-label={t('workspace.gitPanel.ignoreNamed', { path: file.path })}
                    title={t('workspace.gitPanel.addGitignore')}
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                ) : null}
              </li>
            )
          })}
        </ul>
      ) : null}
    </section>
  )
}

function CommitSection({
  action,
  commits,
  defaultExpanded,
  hasMore,
  loadingMore,
  onLoadMore,
  onSelect,
  selectedCommit,
  title,
}: {
  action?: ReactNode
  commits: GroupWorkspaceGitCommitSummary[]
  defaultExpanded: boolean
  hasMore?: boolean
  loadingMore?: boolean
  onLoadMore?: () => void
  onSelect: (sha: string) => void
  selectedCommit: string | undefined
  title: string
}) {
  const { t } = useTranslation('chat')
  const [expanded, setExpanded] = useState(defaultExpanded)
  return (
    <section className="border-b border-border/70">
      <CollapsibleHeader
        title={title}
        expanded={expanded}
        onToggle={() => setExpanded((value) => !value)}
        action={action}
      />
      {expanded ? (
        <div className="pb-1">
          {commits.map((item) => (
            <button
              key={item.sha}
              type="button"
              className={cn(
                'flex h-9 w-full min-w-0 items-center gap-2 px-3 text-left hover:bg-muted/70',
                selectedCommit === item.sha && 'bg-muted',
              )}
              onClick={() => onSelect(item.sha)}
            >
              <GitCommitHorizontal className={cn('h-4 w-4 shrink-0', item.local_only ? 'text-emerald-500' : 'text-muted-foreground')} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs font-medium">{item.subject}</span>
                <span className="block truncate font-mono text-[10px] text-muted-foreground">{item.short_sha} · {item.author_name}</span>
              </span>
              {item.local_only ? <ArrowUp className="h-3.5 w-3.5 shrink-0 text-emerald-500" /> : null}
            </button>
          ))}
          {hasMore && onLoadMore ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="mx-2 h-7 text-xs"
              disabled={loadingMore}
              onClick={onLoadMore}
            >
              {t('workspace.gitPanel.loadMore')}
            </Button>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}

function GitActionItem({
  danger,
  disabled,
  hint,
  label,
  onClick,
}: {
  danger?: boolean
  disabled?: boolean
  hint?: string
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      className={cn(
        'flex w-full flex-col items-start px-3 py-1 text-left hover:bg-muted disabled:pointer-events-none disabled:opacity-40',
        danger && 'text-destructive',
      )}
      onClick={onClick}
    >
      <span className="text-xs font-medium">{label}</span>
      {hint ? <span className="text-[10px] text-muted-foreground">{hint}</span> : null}
    </button>
  )
}

function pullRequestUrl(remoteUrl: string | null, branch: string | null) {
  if (!remoteUrl || !branch) return null
  let host = ''
  let path = ''
  try {
    const parsed = new URL(remoteUrl)
    if (!['http:', 'https:', 'ssh:'].includes(parsed.protocol)) return null
    host = parsed.host
    path = parsed.pathname
  } catch {
    const scp = remoteUrl.match(/^(?:[^@]+@)?([^:]+):(.+)$/)
    if (!scp) return null
    host = scp[1] ?? ''
    path = scp[2] ?? ''
  }
  path = path.replace(/^\/+|\/+$/g, '').replace(/\.git$/i, '')
  if (!host || path.split('/').length < 2) return null
  const encodedBranch = branch.split('/').map(encodeURIComponent).join('/')
  if (host.toLowerCase().includes('github')) {
    return `https://${host}/${path}/pull/new/${encodedBranch}`
  }
  if (host.toLowerCase().includes('gitlab')) {
    return `https://${host}/${path}/-/merge_requests/new?merge_request[source_branch]=${encodeURIComponent(branch)}`
  }
  if (host.toLowerCase().includes('bitbucket')) {
    return `https://${host}/${path}/pull-requests/new?source=${encodeURIComponent(branch)}`
  }
  return null
}

async function openExternal(url: string) {
  if (isDesktopRuntime()) {
    const shell = await import('@tauri-apps/plugin-shell')
    await shell.open(url)
    return
  }
  window.open(url, '_blank', 'noopener,noreferrer')
}

export function WorkspaceGitTab({ groupId, scope = 'groups' }: WorkspaceGitTabProps) {
  const { t, i18n } = useTranslation(['chat', 'common'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const [reviewMode, setReviewMode] = useState<ReviewMode>('changes')
  const [selection, setSelection] = useState<ChangeSelection>(null)
  const [selectedCommit, setSelectedCommit] = useState<string | undefined>()
  const [reviewOpen, setReviewOpen] = useState(false)
  const [commitMessage, setCommitMessage] = useState('')
  const [commitPrompt, setCommitPrompt] = useState(readCommitPrompt)
  const [gitError, setGitError] = useState<string | null>(null)
  const [branchSheetOpen, setBranchSheetOpen] = useState(false)
  const [remoteDialogOpen, setRemoteDialogOpen] = useState(false)
  const [remoteUrl, setRemoteUrl] = useState('')
  const [pendingRemoteOperation, setPendingRemoteOperation] = useState<RemoteOperation>(null)
  const [discardTarget, setDiscardTarget] = useState<GroupWorkspaceGitFileStatus | null>(null)
  const [discardAllOpen, setDiscardAllOpen] = useState(false)
  const [forcePushOpen, setForcePushOpen] = useState(false)
  const [historySkip, setHistorySkip] = useState(0)
  const [history, setHistory] = useState<GroupWorkspaceGitCommitSummary[]>([])
  const [branchFromCommit, setBranchFromCommit] = useState('')
  const [actionsOpen, setActionsOpen] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const actionsRef = useRef<HTMLDivElement>(null)

  const status = useGroupWorkspaceGitStatus(groupId)
  const branchDiff = useGroupWorkspaceGitDiff(
    status.data?.upstream && (status.data.ahead ?? 0) > 0 ? groupId : undefined,
    'branch',
  )
  const diff = useGroupWorkspaceGitDiff(selection ? groupId : undefined, selection?.mode ?? 'worktree', selection?.path)
  const log = useGroupWorkspaceGitLog(groupId, { limit: 50, skip: historySkip })
  const commit = useGroupWorkspaceGitCommit(groupId, selectedCommit)
  const commitDiff = useGroupWorkspaceGitCommitDiff(groupId, selectedCommit)
  const stage = useStageGroupWorkspaceGit(groupId)
  const unstage = useUnstageGroupWorkspaceGit(groupId)
  const commitChanges = useCommitGroupWorkspaceGit(groupId)
  const generateMessage = useGenerateGroupWorkspaceGitCommitMessage(groupId)
  const pull = usePullGroupWorkspaceGit(groupId, scope)
  const push = usePushGroupWorkspaceGit(groupId)
  const forcePush = useForcePushGroupWorkspaceGit(groupId)
  const rebase = useRebaseGroupWorkspaceGit(groupId, scope)
  const fetch = useFetchGroupWorkspaceGit(groupId)
  const init = useInitGroupWorkspaceGit(groupId)
  const discard = useDiscardGroupWorkspaceGit(groupId, scope)
  const ignore = useIgnoreGroupWorkspaceGit(groupId)
  const setRemote = useSetGroupWorkspaceGitRemote(groupId)
  const createBranchFromCommit = useCreateGroupWorkspaceGitBranchFromCommit(groupId, selectedCommit)

  const hasGroupId = Boolean(groupId)
  const files = status.data?.files ?? []
  const staged = files.filter((file) => file.staged)
  const unstaged = files.filter((file) => file.unstaged || file.untracked)
  const busy = stage.isPending
    || unstage.isPending
    || commitChanges.isPending
    || generateMessage.isPending
    || pull.isPending
    || push.isPending
    || forcePush.isPending
    || rebase.isPending
    || fetch.isPending
    || init.isPending
    || discard.isPending
    || ignore.isPending
    || setRemote.isPending
    || createBranchFromCommit.isPending
  const canUseGit = hasGroupId && status.data?.available === true && !busy
  const hasRemote = Boolean(status.data?.remote_name)
  const hasUpstream = Boolean(status.data?.upstream)
  const canCommit = canUseGit && staged.length > 0 && Boolean(commitMessage.trim())
  const canCommitAndSync = canCommit && hasRemote && hasUpstream && unstaged.length === 0
  const canRewriteRemote = canUseGit && hasRemote && hasUpstream && (status.data?.ahead ?? 0) > 0
  const canUpdateFromRemote = canUseGit && hasRemote && hasUpstream && files.length === 0
  const prUrl = hasUpstream
    ? pullRequestUrl(status.data?.remote_url ?? null, status.data?.branch ?? null)
    : null
  const currentDiff = reviewMode === 'history' ? commitDiff : diff
  const committed = committedFiles(branchDiff.data?.patch ?? '')
  const q = query.trim().toLowerCase()
  const visibleCommitted = committed.filter((file) => !q || file.path.toLowerCase().includes(q))
  const visibleStaged = staged.filter((file) => !q || file.path.toLowerCase().includes(q))
  const visibleUnstaged = unstaged.filter((file) => !q || file.path.toLowerCase().includes(q))
  const visibleHistory = history.filter((item) => !q
    || item.subject.toLowerCase().includes(q)
    || item.author_name.toLowerCase().includes(q)
    || item.short_sha.toLowerCase().includes(q))

  useEffect(() => {
    setHistorySkip(0)
    setHistory([])
    setSelectedCommit(undefined)
    setSelection(null)
    setReviewOpen(false)
  }, [groupId])

  useEffect(() => {
    try {
      localStorage.setItem(COMMIT_PROMPT_STORAGE_KEY, commitPrompt)
    } catch {
      // A custom prompt is a convenience; storage denial must not block Git.
    }
  }, [commitPrompt])

  useEffect(() => {
    if (!log.data) return
    setHistory((current) => {
      if (historySkip === 0) return log.data.commits
      const existing = new Set(current.map((item) => item.sha))
      return [...current, ...log.data.commits.filter((item) => !existing.has(item.sha))]
    })
  }, [historySkip, log.data])

  useEffect(() => {
    if (!actionsOpen) return
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && actionsRef.current?.contains(event.target)) return
      setActionsOpen(false)
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setActionsOpen(false)
    }
    window.addEventListener('pointerdown', dismiss)
    window.addEventListener('keydown', escape)
    return () => {
      window.removeEventListener('pointerdown', dismiss)
      window.removeEventListener('keydown', escape)
    }
  }, [actionsOpen])

  const run = (
    operation: () => Promise<unknown>,
    options?: { clearCommit?: boolean; remote?: boolean },
  ) => {
    setGitError(null)
    void operation()
      .then(() => {
        if (options?.clearCommit) setCommitMessage('')
      })
      .catch((error: unknown) => {
        if (options?.remote && isMissingRemote(error)) {
          setPendingRemoteOperation(() => operation)
          setRemoteUrl(status.data?.remote_url ?? '')
          setRemoteDialogOpen(true)
          return
        }
        setGitError(errorMessage(error))
      })
  }

  const saveRemoteAndRetry = () => {
    run(async () => {
      await setRemote.mutateAsync({ remote_url: remoteUrl.trim() })
      setRemoteDialogOpen(false)
      if (pendingRemoteOperation) await pendingRemoteOperation()
      setPendingRemoteOperation(null)
    })
  }

  const commitThen = async (next?: () => Promise<unknown>) => {
    await commitChanges.mutateAsync({ message: commitMessage.trim() })
    setCommitMessage('')
    if (next) await next()
  }

  const syncGit = async () => {
    await pull.mutateAsync({})
    await push.mutateAsync({})
  }

  const closeActionsAndRun = (
    operation: () => Promise<unknown>,
    options?: { clearCommit?: boolean; remote?: boolean },
  ) => {
    setActionsOpen(false)
    run(operation, options)
  }

  const openReview = (nextMode: ReviewMode, nextSelection?: NonNullable<ChangeSelection>, sha?: string) => {
    setReviewMode(nextMode)
    if (nextSelection) setSelection(nextSelection)
    if (sha) setSelectedCommit(sha)
    setReviewOpen(true)
  }

  const primaryAction = staged.length > 0
    ? {
        disabled: !canCommit,
        icon: <GitCommitHorizontal className="h-3.5 w-3.5" />,
        label: t('gitOrcaActions.commit'),
        run: () => run(() => commitThen()),
      }
    : !hasUpstream && hasRemote
      ? {
          disabled: !canUseGit,
          icon: <ArrowUp className="h-3.5 w-3.5" />,
          label: t('gitOrcaActions.publishBranch'),
          run: () => run(() => push.mutateAsync({}), { remote: true }),
        }
      : (status.data?.ahead ?? 0) > 0
        ? {
            disabled: !canUseGit,
            icon: <ArrowUp className="h-3.5 w-3.5" />,
            label: t('workspace.gitPanel.push'),
            run: () => run(() => push.mutateAsync({}), { remote: true }),
          }
        : (status.data?.behind ?? 0) > 0
          ? {
              disabled: !canUpdateFromRemote,
              icon: <ArrowDown className="h-3.5 w-3.5" />,
              label: t('workspace.gitPanel.pull'),
              run: () => run(() => pull.mutateAsync({}), { remote: true }),
            }
          : {
              disabled: !canUpdateFromRemote,
              icon: <RefreshCw className="h-3.5 w-3.5" />,
              label: t('gitOrcaActions.sync'),
              run: () => run(syncGit, { remote: true }),
            }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <header className="relative z-10 shrink-0 space-y-1.5 border-b border-border p-2">
        <div className="flex items-center gap-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 rounded-md bg-foreground px-2 text-xs text-background hover:bg-foreground/90 hover:text-background"
            disabled={!prUrl || !canUseGit}
            onClick={() => {
              if (prUrl) void openExternal(prUrl).catch((error) => setGitError(errorMessage(error)))
            }}
          >
            <GitPullRequest className="h-3.5 w-3.5" />
            {t('gitOrcaActions.createPr')}
          </Button>
          <div className="ml-auto flex items-center gap-0.5">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              aria-label={t('gitOrcaActions.search')}
              aria-expanded={searchOpen}
              onClick={() => {
                setSearchOpen((value) => !value)
                if (searchOpen) setQuery('')
              }}
            >
              <Search className="h-3.5 w-3.5" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              disabled={!hasGroupId}
              aria-label={t('workspace.gitPanel.manageBranches')}
              onClick={() => setBranchSheetOpen(true)}
            >
              <MoreHorizontal className="h-4 w-4" />
            </Button>
          </div>
        </div>

        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-md px-1 py-0.5 text-left hover:bg-muted/60"
          disabled={!hasGroupId}
          onClick={() => setBranchSheetOpen(true)}
          title={t('workspace.gitPanel.manageBranches')}
        >
          <GitBranch className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate font-mono text-xs font-semibold">
            {status.data?.branch ?? 'Git'}
          </span>
          <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
        </button>
        <div className="flex min-h-4 items-center gap-1 px-1 text-[10px] text-muted-foreground">
          <ArrowRight className="h-3 w-3 shrink-0" />
          <span className="min-w-0 flex-1 truncate font-mono">
            {status.data?.upstream ?? status.data?.remote_name ?? t('workspace.gitPanel.noRemote')}
          </span>
          {(status.data?.behind ?? 0) > 0 ? (
            <span className="inline-flex items-center text-amber-500">
              <ArrowDown className="h-3 w-3" />{formatNumber(status.data?.behind ?? 0, language)}
            </span>
          ) : null}
          {(status.data?.ahead ?? 0) > 0 ? (
            <span className="inline-flex items-center text-emerald-500">
              <ArrowUp className="h-3 w-3" />{formatNumber(status.data?.ahead ?? 0, language)}
            </span>
          ) : null}
          {prUrl ? (
            <button
              type="button"
              className="rounded p-0.5 hover:bg-muted hover:text-foreground"
              aria-label={t('gitOrcaActions.createPr')}
              onClick={() => void openExternal(prUrl).catch((error) => setGitError(errorMessage(error)))}
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </div>
        <p className="px-1 text-[10px] leading-3 text-muted-foreground">{statusSummary(status.data, t, language)}</p>

        {searchOpen ? (
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              autoFocus
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Escape') {
                  setQuery('')
                  setSearchOpen(false)
                }
              }}
              placeholder={t('gitOrcaActions.searchPlaceholder')}
              aria-label={t('gitOrcaActions.search')}
              className="h-7 px-8 text-xs"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="absolute right-0.5 top-1/2 h-6 w-6 -translate-y-1/2"
              aria-label={t('common:actions.close')}
              onClick={() => {
                setQuery('')
                setSearchOpen(false)
              }}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        ) : null}

        <div className="relative">
          <Textarea
            value={commitMessage}
            onChange={(event) => setCommitMessage(event.target.value)}
            placeholder={t('chat:workspace.commitMessage')}
            className="min-h-11 resize-none rounded-md py-1.5 pr-8 text-xs"
            rows={2}
            disabled={!canUseGit}
            aria-label={t('chat:workspace.commitMessage')}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="absolute right-1 top-1 h-6 w-6"
            disabled={!canUseGit || staged.length === 0}
            onClick={() => run(() => generateMessage.mutateAsync({
              ...(commitPrompt.trim() ? { prompt: commitPrompt.trim() } : {}),
            }).then((result) => setCommitMessage(result.message)))}
            aria-label={t('chat:workspace.gitPanel.generateCommitMessage')}
            title={t('chat:workspace.gitPanel.generateCommitMessage')}
          >
            <Sparkles className={cn('h-3.5 w-3.5', generateMessage.isPending && 'animate-pulse')} />
          </Button>
        </div>

        <details className="rounded-md border border-border/70 bg-muted/20 px-2 py-1">
          <summary className="cursor-pointer select-none text-[10px] text-muted-foreground">
            {t('chat:workspace.gitPanel.commitPrompt')}
          </summary>
          <Textarea
            value={commitPrompt}
            onChange={(event) => setCommitPrompt(event.target.value)}
            rows={3}
            maxLength={4000}
            className="mt-1 min-h-16 resize-y text-xs"
            placeholder={t('chat:workspace.gitPanel.commitPromptPlaceholder')}
            aria-label={t('chat:workspace.gitPanel.commitPrompt')}
          />
          <p className="py-1 text-[10px] text-muted-foreground">
            {t('chat:workspace.gitPanel.commitPromptHint')}
          </p>
        </details>

        <div ref={actionsRef} className="relative flex overflow-visible rounded-lg border border-border bg-card">
          <button
            type="button"
            className="flex h-7 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-l-lg text-xs font-semibold hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
            disabled={primaryAction.disabled}
            onClick={primaryAction.run}
          >
            {busy ? <LoaderCircle className="h-3.5 w-3.5 animate-spin" /> : primaryAction.icon}
            {primaryAction.label}
          </button>
          <button
            type="button"
            className="flex h-7 w-8 items-center justify-center rounded-r-lg border-l border-border hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
            disabled={!canUseGit}
            aria-haspopup="menu"
            aria-expanded={actionsOpen}
            aria-controls="workspace-git-actions"
            aria-label={t('gitOrcaActions.moreActions')}
            onClick={() => setActionsOpen((value) => !value)}
          >
            <ChevronDown className="h-3.5 w-3.5" />
          </button>

          {actionsOpen ? (
            <div
              id="workspace-git-actions"
              role="menu"
              aria-label={t('gitOrcaActions.actionMenu')}
              className="absolute left-0 right-0 top-full z-30 mt-1 max-h-[min(30rem,60vh)] overflow-y-auto rounded-xl border border-border bg-popover py-1 text-popover-foreground shadow-xl"
            >
              <GitActionItem
                label={t('gitOrcaActions.commit')}
                disabled={!canCommit}
                hint={!canCommit ? t('gitOrcaActions.commitHint') : undefined}
                onClick={() => closeActionsAndRun(() => commitThen())}
              />
              <GitActionItem
                label={t('gitOrcaActions.commitAndPush')}
                disabled={!canCommit || !hasRemote}
                onClick={() => closeActionsAndRun(() => commitThen(() => push.mutateAsync({})))}
              />
              <GitActionItem
                label={t('gitOrcaActions.commitAndSync')}
                disabled={!canCommitAndSync}
                onClick={() => closeActionsAndRun(() => commitThen(syncGit))}
              />
              <div className="my-1 border-t border-border" role="separator" />
              <GitActionItem
                label={t('gitOrcaActions.pushCount', {
                  count: formatNumber(status.data?.ahead ?? 0, language),
                })}
                disabled={!canUseGit || !hasRemote || !hasUpstream}
                onClick={() => closeActionsAndRun(() => push.mutateAsync({}), { remote: true })}
              />
              <GitActionItem
                label={t('gitOrcaActions.forcePushCount', {
                  count: formatNumber(status.data?.ahead ?? 0, language),
                })}
                danger
                disabled={!canRewriteRemote}
                onClick={() => {
                  setActionsOpen(false)
                  setForcePushOpen(true)
                }}
              />
              <GitActionItem
                label={t('gitOrcaActions.createPr')}
                disabled={!prUrl}
                hint={!prUrl ? t('gitOrcaActions.prUnavailable') : undefined}
                onClick={() => {
                  setActionsOpen(false)
                  if (prUrl) void openExternal(prUrl).catch((error) => setGitError(errorMessage(error)))
                }}
              />
              <div className="my-1 border-t border-border" role="separator" />
              <GitActionItem
                label={t('workspace.gitPanel.pull')}
                hint={t('gitOrcaActions.fastForwardHint')}
                disabled={!canUpdateFromRemote}
                onClick={() => closeActionsAndRun(() => pull.mutateAsync({}), { remote: true })}
              />
              <GitActionItem
                label={t('gitOrcaActions.syncCounts', {
                  behind: formatNumber(status.data?.behind ?? 0, language),
                  ahead: formatNumber(status.data?.ahead ?? 0, language),
                })}
                disabled={!canUpdateFromRemote}
                onClick={() => closeActionsAndRun(syncGit, { remote: true })}
              />
              <GitActionItem
                label={t('gitOrcaActions.rebaseFrom', {
                  upstream: status.data?.upstream ?? status.data?.remote_name ?? 'remote',
                })}
                disabled={!canUpdateFromRemote || !hasUpstream}
                onClick={() => closeActionsAndRun(() => rebase.mutateAsync({}), { remote: true })}
              />
              <GitActionItem
                label={t('workspace.gitPanel.fetch')}
                disabled={!canUseGit || !hasRemote}
                onClick={() => closeActionsAndRun(() => fetch.mutateAsync({}), { remote: true })}
              />
              <GitActionItem
                label={t('gitOrcaActions.publishBranch')}
                disabled={!canUseGit || !hasRemote || hasUpstream}
                onClick={() => closeActionsAndRun(() => push.mutateAsync({}), { remote: true })}
              />
            </div>
          ) : null}
        </div>
      </header>

      {gitError || status.error ? (
        <p className="shrink-0 border-b border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">
          {t('chat:workspace.gitPanel.errorDetail', { message: gitError ?? errorMessage(status.error) })}
        </p>
      ) : null}
      {!hasGroupId ? <p className="p-3 text-sm text-muted-foreground">{t('chat:workspace.gitPanel.selectGroup')}</p> : null}
      {hasGroupId && status.isLoading ? (
        <p className="flex items-center gap-2 p-3 text-sm text-muted-foreground">
          <LoaderCircle className="h-4 w-4 animate-spin" /> {t('chat:workspace.gitPanel.loading')}
        </p>
      ) : null}
      {status.data?.available === false ? (
        <div className="m-3 space-y-3 rounded-md border border-border bg-muted/50 p-3">
          <p className="text-xs text-muted-foreground">
            {status.data.message
              ? t('chat:workspace.gitPanel.unavailableDetail', { message: status.data.message })
              : t('chat:workspace.noRepository')}
          </p>
          <Button type="button" size="sm" disabled={init.isPending} onClick={() => run(() => init.mutateAsync({}))}>
            <GitBranch className="h-3.5 w-3.5" />
            {init.isPending ? t('chat:workspace.gitPanel.initializing') : t('chat:workspace.gitPanel.initialize')}
          </Button>
        </div>
      ) : null}

      {status.data?.available === true ? (
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
          <ChangeSection
            title={t('gitOrcaActions.committedChanges')}
            files={visibleCommitted}
            selection={selection}
            action={null}
            disabled={!canUseGit}
            onSelect={(next) => openReview('changes', next)}
          />
          <ChangeSection
            title={t('gitOrcaActions.stagedChanges')}
            files={visibleStaged}
            selection={selection}
            action="unstage"
            disabled={!canUseGit}
            onSelect={(next) => openReview('changes', next)}
            onAction={(paths) => run(() => unstage.mutateAsync({ paths }))}
          />
          <ChangeSection
            title={t('gitOrcaActions.workingChanges')}
            files={visibleUnstaged}
            selection={selection}
            action="stage"
            disabled={!canUseGit}
            onSelect={(next) => openReview('changes', next)}
            onAction={(paths) => run(() => stage.mutateAsync({ paths }))}
            onDiscard={setDiscardTarget}
            onIgnore={(file) => run(() => ignore.mutateAsync({ path: file.path }))}
          />
          {q && visibleCommitted.length === 0 && visibleStaged.length === 0 && visibleUnstaged.length === 0 && visibleHistory.length === 0 ? (
            <p className="p-3 text-xs text-muted-foreground">{t('gitOrcaActions.noMatches')}</p>
          ) : null}
          {!q && files.length === 0 && committed.length === 0 ? (
            <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground">
              <Check className="h-7 w-7 text-emerald-500" />
              <p>{t('chat:workspace.noChanges')}</p>
            </div>
          ) : null}
          {files.length > 0 ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="m-2 h-7 text-xs text-destructive"
              disabled={!canUseGit}
              onClick={() => setDiscardAllOpen(true)}
            >
              <Trash2 className="h-3.5 w-3.5" />
              {t('chat:workspace.gitPanel.discardAllChanges')}
            </Button>
          ) : null}
        </div>
      ) : null}

      {hasGroupId && status.data?.available === true ? (
        <div className="max-h-[45%] shrink-0 overflow-y-auto border-t border-border">
          <CommitSection
            title={t('gitOrcaActions.commits')}
            commits={visibleHistory}
            defaultExpanded={false}
            selectedCommit={selectedCommit}
            onSelect={(sha) => openReview('history', undefined, sha)}
            hasMore={log.data?.has_more}
            loadingMore={log.isFetching}
            onLoadMore={() => setHistorySkip((value) => value + 50)}
            action={(
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                disabled={status.isFetching}
                onClick={() => void status.refetch()}
                aria-label={t('chat:workspace.gitPanel.refreshAria')}
                title={t('chat:workspace.refresh')}
              >
                <RefreshCw className={cn('h-3.5 w-3.5', status.isFetching && 'animate-spin')} />
              </Button>
            )}
          />
        </div>
      ) : null}

      <WorkspaceGitBranchSheet
        groupId={groupId}
        scope={scope}
        open={branchSheetOpen}
        onOpenChange={setBranchSheetOpen}
        onError={setGitError}
        onSetRemote={() => {
          setRemoteUrl(status.data?.remote_url ?? '')
          setRemoteDialogOpen(true)
        }}
      />

      <Dialog open={reviewOpen} onOpenChange={setReviewOpen}>
        <DialogContent closeLabel={t('common:actions.close')} className="flex h-[min(46rem,88vh)] w-[calc(100vw-2rem)] max-w-5xl flex-col gap-0 overflow-hidden p-0">
          <DialogHeader className="shrink-0 border-b border-border px-5 py-4 pr-12">
            <DialogTitle className="truncate text-base">
              {reviewMode === 'history'
                ? commit.data?.subject ?? t('chat:workspace.gitPanel.commitDiff')
                : selection?.path ?? t('chat:workspace.gitPanel.selectChangedFile')}
            </DialogTitle>
            <DialogDescription>
              {reviewMode === 'history' && commit.data
                ? `${commit.data.short_sha} · ${commit.data.author_name} · +${formatNumber(commit.data.insertions, language)} -${formatNumber(commit.data.deletions, language)}`
                : currentDiff.data?.stat || t('chat:workspace.gitPanel.workspaceGit')}
            </DialogDescription>
          </DialogHeader>
          {reviewMode === 'history' && selectedCommit ? (
            <div className="flex shrink-0 gap-2 border-b border-border p-2">
              <Input
                value={branchFromCommit}
                onChange={(event) => setBranchFromCommit(event.target.value)}
                placeholder={t('chat:workspace.gitPanel.branchFromCommit')}
                className="h-8 text-xs"
                aria-label={t('chat:workspace.gitPanel.branchNameFromCommit')}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-8 shrink-0 text-xs"
                disabled={!canUseGit || !branchFromCommit.trim()}
                onClick={() => run(() => createBranchFromCommit.mutateAsync({ name: branchFromCommit.trim() }).then(() => setBranchFromCommit('')))}
              >
                {t('chat:workspace.gitPanel.create')}
              </Button>
            </div>
          ) : null}
          <DiffPatch
            content={currentDiff.isLoading
              ? t('chat:workspace.gitPanel.loadingDiff')
              : currentDiff.data?.patch || t('chat:workspace.gitPanel.noDiff')}
            highlight={!currentDiff.isLoading && Boolean(currentDiff.data?.patch)}
          />
          {currentDiff.data?.truncated ? (
            <p className="shrink-0 border-t border-border px-3 py-1 text-[10px] text-muted-foreground">
              {t('chat:workspace.gitPanel.truncated')}
            </p>
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog open={remoteDialogOpen} onOpenChange={setRemoteDialogOpen}>
        <DialogContent closeLabel={t('common:actions.close')} className="w-[calc(100vw-2rem)] sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t('chat:workspace.gitPanel.setRemoteTitle')}</DialogTitle>
            <DialogDescription>{t('chat:workspace.gitPanel.setRemoteDescription')}</DialogDescription>
          </DialogHeader>
          <Input
            value={remoteUrl}
            onChange={(event) => setRemoteUrl(event.target.value)}
            placeholder={t('chat:workspace.gitPanel.remoteUrlPlaceholder')}
            aria-label={t('chat:workspace.gitPanel.remoteUrl')}
          />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setRemoteDialogOpen(false)}>
              {t('common:actions.cancel')}
            </Button>
            <Button type="button" disabled={!remoteUrl.trim() || setRemote.isPending} onClick={saveRemoteAndRetry}>
              {t('chat:workspace.gitPanel.saveRetry')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={forcePushOpen}
        onOpenChange={setForcePushOpen}
        title={t('gitOrcaActions.forcePushTitle')}
        description={t('gitOrcaActions.forcePushDescription')}
        confirmLabel={t('gitOrcaActions.forcePush')}
        destructive
        onConfirm={async () => {
          await forcePush.mutateAsync({})
        }}
      />
      <ConfirmDialog
        open={discardAllOpen}
        onOpenChange={setDiscardAllOpen}
        title={t('chat:workspace.gitPanel.discardAllTitle')}
        description={t('chat:workspace.gitPanel.discardAllDescription')}
        confirmLabel={t('chat:workspace.gitPanel.discardAll')}
        destructive
        onConfirm={async () => {
          try {
            await discard.mutateAsync({ paths: [], all: true })
          } catch (error: unknown) {
            throw new Error(t('common:workspaceOperations.discardGitError', { message: errorMessage(error) }))
          }
        }}
      />
      <ConfirmDialog
        open={discardTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDiscardTarget(null)
        }}
        title={t('chat:workspace.gitPanel.discardFileTitle')}
        description={discardTarget
          ? t('chat:workspace.gitPanel.discardFileDescription', { path: discardTarget.path })
          : undefined}
        confirmLabel={t('chat:workspace.discard')}
        destructive
        onConfirm={async () => {
          try {
            if (discardTarget) await discard.mutateAsync({ paths: [discardTarget.path], all: false })
          } catch (error: unknown) {
            throw new Error(t('common:workspaceOperations.discardGitError', { message: errorMessage(error) }))
          }
        }}
      />
    </div>
  )
}
