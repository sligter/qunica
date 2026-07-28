import { useEffect, useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronRight,
  FileDiff,
  GitBranch,
  GitCommitHorizontal,
  History,
  LoaderCircle,
  Maximize2,
  Minus,
  Minimize2,
  Plus,
  RefreshCw,
  RotateCcw,
  Sparkles,
  Trash2,
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
  useSetGroupWorkspaceGitRemote,
  useStageGroupWorkspaceGit,
  useUnstageGroupWorkspaceGit,
} from '@/hooks/useWorkspaceGit'
import { normalizeLanguage } from '@/i18n'
import { ApiError } from '@/lib/api-v2/client'
import { formatNumber } from '@/lib/format'
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
type ChangeSelection = { path: string; mode: 'worktree' | 'staged' } | null
type RemoteOperation = (() => Promise<unknown>) | null
type RepositoryState = NonNullable<GroupWorkspaceGitStatus['state']>
type DiffLineKind = 'addition' | 'deletion' | 'hunk' | 'meta' | 'context'

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

function ChangeSection({
  title,
  files,
  selection,
  action,
  disabled,
  onSelect,
  onAction,
  onDiscard,
  onIgnore,
}: {
  title: string
  files: GroupWorkspaceGitFileStatus[]
  selection: ChangeSelection
  action: 'stage' | 'unstage'
  disabled: boolean
  onSelect: (selection: ChangeSelection) => void
  onAction: (paths: string[]) => void
  onDiscard?: (file: GroupWorkspaceGitFileStatus) => void
  onIgnore?: (file: GroupWorkspaceGitFileStatus) => void
}) {
  const { t, i18n } = useTranslation('chat')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  if (files.length === 0) return null
  const diffMode = action === 'stage' ? 'worktree' : 'staged'
  return (
    <section className="border-b border-border">
      <header className="flex items-center justify-between px-3 py-1.5">
        <span className="text-2xs font-medium uppercase text-muted-foreground">
          {title} ({formatNumber(files.length, language)})
        </span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          disabled={disabled}
          onClick={() => onAction([])}
          aria-label={action === 'stage' ? t('workspace.gitPanel.stageAll') : t('workspace.gitPanel.unstageAll')}
          title={action === 'stage' ? t('workspace.gitPanel.stageAll') : t('workspace.gitPanel.unstageAll')}
        >
          {action === 'stage' ? <Plus className="h-3 w-3" /> : <Minus className="h-3 w-3" />}
        </Button>
      </header>
      <ul>
        {files.map((file) => {
          const selected = selection?.path === file.path && selection.mode === diffMode
          return (
            <li
              key={`${title}:${file.path}`}
              className={cn(
                'group flex min-w-0 items-center gap-1 px-2 py-1 hover:bg-muted/70',
                selected && 'bg-muted',
              )}
            >
              <button
                type="button"
                className="flex min-w-0 flex-1 items-center gap-2 text-left"
                title={file.path}
                onClick={() => onSelect({ path: file.path, mode: diffMode })}
              >
                <span className="w-5 shrink-0 font-mono text-[10px] text-muted-foreground">
                  {file.status}
                </span>
                <span className="truncate text-xs">{file.path}</span>
              </button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0"
                disabled={disabled}
                onClick={() => onAction([file.path])}
                aria-label={action === 'stage' ? t('workspace.gitPanel.stageNamed', { path: file.path }) : t('workspace.gitPanel.unstageNamed', { path: file.path })}
                title={action === 'stage' ? t('workspace.stage') : t('workspace.unstage')}
              >
                {action === 'stage' ? <Plus className="h-3 w-3" /> : <Minus className="h-3 w-3" />}
              </Button>
              {onDiscard ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  disabled={disabled}
                  onClick={() => onDiscard(file)}
                  aria-label={t('workspace.gitPanel.discardNamed', { path: file.path })}
                  title={t('workspace.gitPanel.discardChanges')}
                >
                  <Trash2 className="h-3 w-3 text-destructive" />
                </Button>
              ) : null}
              {onIgnore && file.untracked ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  disabled={disabled}
                  onClick={() => onIgnore(file)}
                  aria-label={t('workspace.gitPanel.ignoreNamed', { path: file.path })}
                  title={t('workspace.gitPanel.addGitignore')}
                >
                  <Minus className="h-3 w-3" />
                </Button>
              ) : null}
            </li>
          )
        })}
      </ul>
    </section>
  )
}

export function WorkspaceGitTab({ groupId, scope = 'groups' }: WorkspaceGitTabProps) {
  const { t, i18n } = useTranslation(['chat', 'common'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const [mode, setMode] = useState<ReviewMode>('changes')
  const [selection, setSelection] = useState<ChangeSelection>(null)
  const [selectedCommit, setSelectedCommit] = useState<string | undefined>()
  const [commitMessage, setCommitMessage] = useState('')
  const [gitError, setGitError] = useState<string | null>(null)
  const [branchSheetOpen, setBranchSheetOpen] = useState(false)
  const [remoteDialogOpen, setRemoteDialogOpen] = useState(false)
  const [remoteUrl, setRemoteUrl] = useState('')
  const [pendingRemoteOperation, setPendingRemoteOperation] = useState<RemoteOperation>(null)
  const [discardTarget, setDiscardTarget] = useState<GroupWorkspaceGitFileStatus | null>(null)
  const [discardAllOpen, setDiscardAllOpen] = useState(false)
  const [historySkip, setHistorySkip] = useState(0)
  const [history, setHistory] = useState<GroupWorkspaceGitCommitSummary[]>([])
  const [branchFromCommit, setBranchFromCommit] = useState('')
  const [diffExpanded, setDiffExpanded] = useState(false)

  const status = useGroupWorkspaceGitStatus(groupId)
  const diff = useGroupWorkspaceGitDiff(groupId, selection?.mode ?? 'worktree', selection?.path)
  const log = useGroupWorkspaceGitLog(groupId, { limit: 50, skip: historySkip })
  const commit = useGroupWorkspaceGitCommit(groupId, selectedCommit)
  const commitDiff = useGroupWorkspaceGitCommitDiff(groupId, selectedCommit)
  const stage = useStageGroupWorkspaceGit(groupId)
  const unstage = useUnstageGroupWorkspaceGit(groupId)
  const commitChanges = useCommitGroupWorkspaceGit(groupId)
  const generateMessage = useGenerateGroupWorkspaceGitCommitMessage(groupId)
  const pull = usePullGroupWorkspaceGit(groupId, scope)
  const push = usePushGroupWorkspaceGit(groupId)
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
  const busy = stage.isPending || unstage.isPending || commitChanges.isPending || generateMessage.isPending || pull.isPending || push.isPending || fetch.isPending || init.isPending || discard.isPending || ignore.isPending || setRemote.isPending || createBranchFromCommit.isPending
  const canUseGit = hasGroupId && status.data?.available === true && !busy
  const currentDiff = mode === 'history' && selectedCommit ? commitDiff : diff

  useEffect(() => {
    setHistorySkip(0)
    setHistory([])
    setSelectedCommit(undefined)
    setDiffExpanded(false)
  }, [groupId])

  useEffect(() => {
    if (!log.data) return
    setHistory((current) => {
      if (historySkip === 0) return log.data.commits
      const existing = new Set(current.map((item) => item.sha))
      return [...current, ...log.data.commits.filter((item) => !existing.has(item.sha))]
    })
  }, [historySkip, log.data])

  const run = (operation: () => Promise<unknown>, options?: { clearCommit?: boolean; remote?: boolean }) => {
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

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <header className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b border-border px-3 py-2">
        <Button type="button" variant="ghost" size="sm" className="h-7 min-w-0 gap-1 px-1.5" disabled={!hasGroupId} onClick={() => setBranchSheetOpen(true)} title={t('chat:workspace.gitPanel.manageBranches')}>
          <GitBranch className="h-3.5 w-3.5 shrink-0" />
          <span className="max-w-28 truncate text-xs">{status.data?.branch ?? 'Git'}</span>
          <ChevronRight className="h-3 w-3 text-muted-foreground" />
        </Button>
        <span className="hidden text-[10px] text-muted-foreground sm:inline">{statusSummary(status.data, t, language)}</span>
        <div className="ml-auto flex items-center gap-0.5">
          <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!canUseGit} onClick={() => run(() => fetch.mutateAsync({}), { remote: true })} aria-label={t('chat:workspace.gitPanel.fetchAria')} title={t('chat:workspace.gitPanel.fetch')}>
            <RefreshCw className={cn('h-3.5 w-3.5', fetch.isPending && 'animate-spin')} />
          </Button>
          <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!canUseGit} onClick={() => run(() => pull.mutateAsync({}), { remote: true })} aria-label={t('chat:workspace.gitPanel.pullAria')} title={t('chat:workspace.gitPanel.pull')}><ArrowDown className="h-3.5 w-3.5" /></Button>
          <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!canUseGit} onClick={() => run(() => push.mutateAsync({}), { remote: true })} aria-label={t('chat:workspace.gitPanel.pushAria')} title={t('chat:workspace.gitPanel.push')}><ArrowUp className="h-3.5 w-3.5" /></Button>
          <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!canUseGit || unstaged.length === 0} onClick={() => run(() => stage.mutateAsync({ paths: [] }))} aria-label={t('chat:workspace.gitPanel.stageAll')} title={t('chat:workspace.gitPanel.stageAll')}><Plus className="h-3.5 w-3.5" /></Button>
          <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!hasGroupId || status.isFetching} onClick={() => void status.refetch()} aria-label={t('chat:workspace.gitPanel.refreshAria')} title={t('chat:workspace.refresh')}><RotateCcw className={cn('h-3.5 w-3.5', status.isFetching && 'animate-spin')} /></Button>
        </div>
      </header>

      {gitError || status.error ? <p className="shrink-0 border-b border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">{t('chat:workspace.gitPanel.errorDetail', { message: gitError ?? errorMessage(status.error) })}</p> : null}
      {!hasGroupId ? <p className="p-3 text-sm text-muted-foreground">{t('chat:workspace.gitPanel.selectGroup')}</p> : null}
      {hasGroupId && status.isLoading ? <p className="flex items-center gap-2 p-3 text-sm text-muted-foreground"><LoaderCircle className="h-4 w-4 animate-spin" /> {t('chat:workspace.gitPanel.loading')}</p> : null}
      {status.data?.available === false ? <div className="m-3 space-y-3 rounded-md border border-border bg-muted/50 p-3"><p className="text-xs text-muted-foreground">{status.data.message ? t('chat:workspace.gitPanel.unavailableDetail', { message: status.data.message }) : t('chat:workspace.noRepository')}</p><Button type="button" size="sm" disabled={init.isPending} onClick={() => run(() => init.mutateAsync({}))}><GitBranch className="h-3.5 w-3.5" />{init.isPending ? t('chat:workspace.gitPanel.initializing') : t('chat:workspace.gitPanel.initialize')}</Button></div> : null}

      {status.data?.available === true ? <>
        <div className="flex shrink-0 border-b border-border px-2">
          <button type="button" className={cn('flex h-8 items-center gap-1 border-b-2 px-2 text-xs', mode === 'changes' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground')} onClick={() => setMode('changes')}><FileDiff className="h-3.5 w-3.5" /> {t('chat:workspace.changes')}</button>
          <button type="button" className={cn('flex h-8 items-center gap-1 border-b-2 px-2 text-xs', mode === 'history' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground')} onClick={() => setMode('history')}><History className="h-3.5 w-3.5" /> {t('chat:workspace.gitPanel.history')}</button>
          <span className="ml-auto self-center text-[10px] text-muted-foreground">{status.data.remote_name ?? t('chat:workspace.gitPanel.noRemote')}{status.data.dirty_counts.staged ? ` / ${t('chat:gitActions.stagedCount', { count: status.data.dirty_counts.staged, formattedCount: formatNumber(status.data.dirty_counts.staged, language) })}` : ''}</span>
        </div>
        <div className={cn(
          'grid min-h-0 flex-1',
          diffExpanded
            ? 'grid-cols-1 grid-rows-1'
            : 'grid-rows-[minmax(12rem,1fr)_minmax(12rem,1fr)] min-[500px]:grid-cols-[minmax(13rem,38%)_1fr] min-[500px]:grid-rows-1',
        )}>
          {!diffExpanded ? (
            <div className="min-h-0 overflow-y-auto border-b border-border min-[500px]:border-b-0 min-[500px]:border-r">
              {mode === 'changes' ? <>
                <ChangeSection title={t('chat:workspace.staged')} files={staged} selection={selection} action="unstage" disabled={!canUseGit} onSelect={setSelection} onAction={(paths) => run(() => unstage.mutateAsync({ paths }))} />
                <ChangeSection title={t('chat:workspace.changes')} files={unstaged} selection={selection} action="stage" disabled={!canUseGit} onSelect={setSelection} onAction={(paths) => run(() => stage.mutateAsync({ paths }))} onDiscard={setDiscardTarget} onIgnore={(file) => run(() => ignore.mutateAsync({ path: file.path }))} />
                {files.length === 0 ? <p className="p-3 text-xs text-muted-foreground">{t('chat:workspace.noChanges')}</p> : <Button type="button" variant="ghost" size="sm" className="m-2 h-7 text-xs text-destructive" disabled={!canUseGit} onClick={() => setDiscardAllOpen(true)}><Trash2 className="h-3.5 w-3.5" /> {t('chat:workspace.gitPanel.discardAllChanges')}</Button>}
              </> : <>
                {log.isLoading && history.length === 0 ? <p className="p-3 text-xs text-muted-foreground">{t('chat:workspace.gitPanel.loadingHistory')}</p> : null}
                {history.map((item) => <button key={item.sha} type="button" className={cn('flex w-full min-w-0 flex-col gap-0.5 border-b border-border px-3 py-2 text-left hover:bg-muted/70', selectedCommit === item.sha && 'bg-muted')} onClick={() => setSelectedCommit(item.sha)}><span className="truncate text-xs font-medium">{item.subject}</span><span className="truncate font-mono text-[10px] text-muted-foreground">{item.short_sha} / {item.author_name}</span></button>)}
                {log.data?.has_more ? <Button type="button" variant="ghost" size="sm" className="m-2 h-7 text-xs" disabled={log.isFetching} onClick={() => setHistorySkip((value) => value + 50)}>{t('chat:workspace.gitPanel.loadMore')}</Button> : null}
              </>}
            </div>
          ) : null}
          <section className="flex min-h-0 flex-col overflow-hidden">
            {mode === 'history' && selectedCommit && commit.data ? <div className="shrink-0 border-b border-border px-3 py-2"><div className="flex items-start gap-2"><GitCommitHorizontal className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" /><div className="min-w-0"><p className="truncate text-xs font-medium">{commit.data.subject}</p><p className="truncate text-[10px] text-muted-foreground">{commit.data.short_sha} / {commit.data.author_name} / +{formatNumber(commit.data.insertions, language)} -{formatNumber(commit.data.deletions, language)}</p></div></div><p className="mt-2 whitespace-pre-wrap text-xs text-muted-foreground">{commit.data.body}</p><div className="mt-2 flex gap-1"><Input value={branchFromCommit} onChange={(event) => setBranchFromCommit(event.target.value)} placeholder={t('chat:workspace.gitPanel.branchFromCommit')} className="h-7 text-xs" aria-label={t('chat:workspace.gitPanel.branchNameFromCommit')} /><Button type="button" variant="outline" size="sm" className="h-7 shrink-0 text-xs" disabled={!canUseGit || !branchFromCommit.trim()} onClick={() => run(() => createBranchFromCommit.mutateAsync({ name: branchFromCommit.trim() }).then(() => setBranchFromCommit('')))}>{t('chat:workspace.gitPanel.create')}</Button></div></div> : null}
            <div className="flex min-h-0 flex-1 flex-col">
              <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1">
                <span className="min-w-0 flex-1 truncate text-2xs text-muted-foreground">{mode === 'history' ? selectedCommit ? t('chat:workspace.gitPanel.commitDiff') : t('chat:workspace.gitPanel.selectCommit') : selection?.path ?? t('chat:workspace.gitPanel.selectChangedFile')}</span>
                {currentDiff.data?.truncated ? <span className="text-[10px] text-muted-foreground">{t('chat:workspace.gitPanel.truncated')}</span> : null}
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6 shrink-0"
                  onClick={() => setDiffExpanded((value) => !value)}
                  aria-label={t(diffExpanded ? 'chat:gitActions.collapseDiff' : 'chat:gitActions.expandDiff')}
                  aria-expanded={diffExpanded}
                  title={t(diffExpanded ? 'chat:gitActions.collapseDiff' : 'chat:gitActions.expandDiff')}
                >
                  {diffExpanded ? <Minimize2 className="h-3.5 w-3.5" /> : <Maximize2 className="h-3.5 w-3.5" />}
                </Button>
              </div>
              <DiffPatch
                content={currentDiff.isLoading ? t('chat:workspace.gitPanel.loadingDiff') : currentDiff.data?.patch || t('chat:workspace.gitPanel.noDiff')}
                highlight={!currentDiff.isLoading && Boolean(currentDiff.data?.patch)}
              />
            </div>
            {mode === 'changes' ? <form className="shrink-0 border-t border-border p-2" onSubmit={(event) => { event.preventDefault(); if (commitMessage.trim()) run(() => commitChanges.mutateAsync({ message: commitMessage.trim() }), { clearCommit: true }) }}><div className="flex items-end gap-1"><Textarea value={commitMessage} onChange={(event) => setCommitMessage(event.target.value)} placeholder={t('chat:workspace.commitMessage')} className="min-h-8 resize-none py-1.5 text-xs" rows={2} disabled={!canUseGit || staged.length === 0} aria-label={t('chat:workspace.commitMessage')} /><div className="flex shrink-0 flex-col gap-1"><Button type="button" variant="outline" size="icon" className="h-7 w-7" disabled={!canUseGit || staged.length === 0} onClick={() => run(() => generateMessage.mutateAsync().then((result) => setCommitMessage(result.message)))} aria-label={t('chat:workspace.gitPanel.generateCommitMessage')} title={t('chat:workspace.gitPanel.generateCommitMessage')}><Sparkles className={cn('h-3.5 w-3.5', generateMessage.isPending && 'animate-pulse')} /></Button><Button type="submit" size="icon" className="h-7 w-7" disabled={!canUseGit || staged.length === 0 || !commitMessage.trim()} aria-label={t('chat:workspace.gitPanel.commitStaged')} title={t('chat:workspace.commit')}><Check className="h-3.5 w-3.5" /></Button></div></div></form> : null}
          </section>
        </div>
      </> : null}

      <WorkspaceGitBranchSheet groupId={groupId} scope={scope} open={branchSheetOpen} onOpenChange={setBranchSheetOpen} onError={setGitError} onSetRemote={() => { setRemoteUrl(status.data?.remote_url ?? ''); setRemoteDialogOpen(true) }} />
      <Dialog open={remoteDialogOpen} onOpenChange={setRemoteDialogOpen}><DialogContent closeLabel={t('common:actions.close')} className="w-[calc(100vw-2rem)] sm:max-w-md"><DialogHeader><DialogTitle>{t('chat:workspace.gitPanel.setRemoteTitle')}</DialogTitle><DialogDescription>{t('chat:workspace.gitPanel.setRemoteDescription')}</DialogDescription></DialogHeader><Input value={remoteUrl} onChange={(event) => setRemoteUrl(event.target.value)} placeholder={t('chat:workspace.gitPanel.remoteUrlPlaceholder')} aria-label={t('chat:workspace.gitPanel.remoteUrl')} /><DialogFooter><Button type="button" variant="outline" onClick={() => setRemoteDialogOpen(false)}>{t('common:actions.cancel')}</Button><Button type="button" disabled={!remoteUrl.trim() || setRemote.isPending} onClick={saveRemoteAndRetry}>{t('chat:workspace.gitPanel.saveRetry')}</Button></DialogFooter></DialogContent></Dialog>
      <ConfirmDialog open={discardAllOpen} onOpenChange={setDiscardAllOpen} title={t('chat:workspace.gitPanel.discardAllTitle')} description={t('chat:workspace.gitPanel.discardAllDescription')} confirmLabel={t('chat:workspace.gitPanel.discardAll')} destructive onConfirm={async () => { try { await discard.mutateAsync({ paths: [], all: true }) } catch (error: unknown) { throw new Error(t('common:workspaceOperations.discardGitError', { message: errorMessage(error) })) } }} />
      <ConfirmDialog open={discardTarget !== null} onOpenChange={(open) => { if (!open) setDiscardTarget(null) }} title={t('chat:workspace.gitPanel.discardFileTitle')} description={discardTarget ? t('chat:workspace.gitPanel.discardFileDescription', { path: discardTarget.path }) : undefined} confirmLabel={t('chat:workspace.discard')} destructive onConfirm={async () => { try { if (discardTarget) await discard.mutateAsync({ paths: [discardTarget.path], all: false }) } catch (error: unknown) { throw new Error(t('common:workspaceOperations.discardGitError', { message: errorMessage(error) })) } }} />
    </div>
  )
}
