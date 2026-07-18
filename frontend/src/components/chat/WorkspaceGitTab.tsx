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
  Minus,
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
  usePopGroupWorkspaceGitStash,
  usePullGroupWorkspaceGit,
  usePushGroupWorkspaceGit,
  usePushGroupWorkspaceGitStash,
  useSetGroupWorkspaceGitRemote,
  useStageGroupWorkspaceGit,
  useUnstageGroupWorkspaceGit,
} from '@/hooks/useWorkspaceGit'
import { normalizeLanguage } from '@/i18n'
import { ApiError } from '@/lib/api-v2/client'
import { formatNumber } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { GroupWorkspaceGitCommitSummary, GroupWorkspaceGitFileStatus } from '@/types/api'

interface WorkspaceGitTabProps {
  groupId: string | undefined
}

type ReviewMode = 'changes' | 'history'
type ChangeSelection = { path: string; mode: 'worktree' | 'staged' } | null
type RemoteOperation = (() => Promise<unknown>) | null

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function isMissingRemote(error: unknown) {
  return error instanceof ApiError && error.code === 'missing_remote'
}

function statusSummary(
  status: ReturnType<typeof useGroupWorkspaceGitStatus>['data'],
  t: TFunction<'chat'>,
  language: 'en-US' | 'zh-CN',
) {
  if (!status) return t('workspace.gitPanel.workspaceGit')
  if (status.state === 'conflict') return t('workspace.gitPanel.conflicts')
  if (status.state === 'detached') return t('workspace.gitPanel.detached')
  if (status.state === 'initial') return t('workspace.gitPanel.initial')
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
        <span className="text-[11px] font-medium uppercase text-muted-foreground">
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

export function WorkspaceGitTab({ groupId }: WorkspaceGitTabProps) {
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

  const status = useGroupWorkspaceGitStatus(groupId)
  const diff = useGroupWorkspaceGitDiff(groupId, selection?.mode ?? 'worktree', selection?.path)
  const log = useGroupWorkspaceGitLog(groupId, { limit: 50, skip: historySkip })
  const commit = useGroupWorkspaceGitCommit(groupId, selectedCommit)
  const commitDiff = useGroupWorkspaceGitCommitDiff(groupId, selectedCommit)
  const stage = useStageGroupWorkspaceGit(groupId)
  const unstage = useUnstageGroupWorkspaceGit(groupId)
  const commitChanges = useCommitGroupWorkspaceGit(groupId)
  const generateMessage = useGenerateGroupWorkspaceGitCommitMessage(groupId)
  const pull = usePullGroupWorkspaceGit(groupId)
  const push = usePushGroupWorkspaceGit(groupId)
  const fetch = useFetchGroupWorkspaceGit(groupId)
  const init = useInitGroupWorkspaceGit(groupId)
  const discard = useDiscardGroupWorkspaceGit(groupId)
  const ignore = useIgnoreGroupWorkspaceGit(groupId)
  const setRemote = useSetGroupWorkspaceGitRemote(groupId)
  const stashPush = usePushGroupWorkspaceGitStash(groupId)
  const stashPop = usePopGroupWorkspaceGitStash(groupId)
  const createBranchFromCommit = useCreateGroupWorkspaceGitBranchFromCommit(groupId, selectedCommit)

  const hasGroupId = Boolean(groupId)
  const files = status.data?.files ?? []
  const staged = files.filter((file) => file.staged)
  const unstaged = files.filter((file) => file.unstaged || file.untracked)
  const busy = stage.isPending || unstage.isPending || commitChanges.isPending || generateMessage.isPending || pull.isPending || push.isPending || fetch.isPending || init.isPending || discard.isPending || ignore.isPending || setRemote.isPending || stashPush.isPending || stashPop.isPending || createBranchFromCommit.isPending
  const canUseGit = hasGroupId && status.data?.available === true && !busy
  const currentDiff = mode === 'history' && selectedCommit ? commitDiff : diff

  useEffect(() => {
    setHistorySkip(0)
    setHistory([])
    setSelectedCommit(undefined)
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
          <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!canUseGit} onClick={() => run(() => stashPush.mutateAsync({}))} aria-label={t('chat:workspace.gitPanel.stashAria')} title={t('chat:workspace.gitPanel.stash')}><GitCommitHorizontal className="h-3.5 w-3.5" /></Button>
          {status.data?.stash_count ? <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={!canUseGit} onClick={() => run(() => stashPop.mutateAsync({}))} aria-label={t('chat:workspace.gitPanel.popStashAria')} title={t('chat:workspace.gitPanel.popStash')}><RotateCcw className="h-3.5 w-3.5" /></Button> : null}
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
          <span className="ml-auto self-center text-[10px] text-muted-foreground">{status.data.remote_name ?? t('chat:workspace.gitPanel.noRemote')}{status.data.stash_count ? ` / ${t('chat:workspace.gitPanel.stashCount', { count: status.data.stash_count, formattedCount: formatNumber(status.data.stash_count, language) })}` : ''}</span>
        </div>
        <div className="grid min-h-0 flex-1 grid-rows-[minmax(12rem,1fr)_minmax(12rem,1fr)] min-[500px]:grid-cols-[minmax(13rem,38%)_1fr] min-[500px]:grid-rows-1">
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
          <section className="flex min-h-0 flex-col overflow-hidden">
            {mode === 'history' && selectedCommit && commit.data ? <div className="shrink-0 border-b border-border px-3 py-2"><div className="flex items-start gap-2"><GitCommitHorizontal className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" /><div className="min-w-0"><p className="truncate text-xs font-medium">{commit.data.subject}</p><p className="truncate text-[10px] text-muted-foreground">{commit.data.short_sha} / {commit.data.author_name} / +{formatNumber(commit.data.insertions, language)} -{formatNumber(commit.data.deletions, language)}</p></div></div><p className="mt-2 whitespace-pre-wrap text-xs text-muted-foreground">{commit.data.body}</p><div className="mt-2 flex gap-1"><Input value={branchFromCommit} onChange={(event) => setBranchFromCommit(event.target.value)} placeholder={t('chat:workspace.gitPanel.branchFromCommit')} className="h-7 text-xs" aria-label={t('chat:workspace.gitPanel.branchNameFromCommit')} /><Button type="button" variant="outline" size="sm" className="h-7 shrink-0 text-xs" disabled={!canUseGit || !branchFromCommit.trim()} onClick={() => run(() => createBranchFromCommit.mutateAsync({ name: branchFromCommit.trim() }).then(() => setBranchFromCommit('')))}>{t('chat:workspace.gitPanel.create')}</Button></div></div> : null}
            <div className="flex min-h-0 flex-1 flex-col"><div className="flex shrink-0 items-center justify-between border-b border-border px-3 py-1.5"><span className="truncate text-[11px] text-muted-foreground">{mode === 'history' ? selectedCommit ? t('chat:workspace.gitPanel.commitDiff') : t('chat:workspace.gitPanel.selectCommit') : selection?.path ?? t('chat:workspace.gitPanel.selectChangedFile')}</span>{currentDiff.data?.truncated ? <span className="text-[10px] text-muted-foreground">{t('chat:workspace.gitPanel.truncated')}</span> : null}</div><pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-[11px] leading-5">{currentDiff.isLoading ? t('chat:workspace.gitPanel.loadingDiff') : currentDiff.data?.patch || t('chat:workspace.gitPanel.noDiff')}</pre></div>
            {mode === 'changes' ? <form className="shrink-0 border-t border-border p-2" onSubmit={(event) => { event.preventDefault(); if (commitMessage.trim()) run(() => commitChanges.mutateAsync({ message: commitMessage.trim() }), { clearCommit: true }) }}><div className="flex items-end gap-1"><Textarea value={commitMessage} onChange={(event) => setCommitMessage(event.target.value)} placeholder={t('chat:workspace.commitMessage')} className="min-h-8 resize-none py-1.5 text-xs" rows={2} disabled={!canUseGit || staged.length === 0} aria-label={t('chat:workspace.commitMessage')} /><div className="flex shrink-0 flex-col gap-1"><Button type="button" variant="outline" size="icon" className="h-7 w-7" disabled={!canUseGit || staged.length === 0} onClick={() => run(() => generateMessage.mutateAsync().then((result) => setCommitMessage(result.message)))} aria-label={t('chat:workspace.gitPanel.generateCommitMessage')} title={t('chat:workspace.gitPanel.generateCommitMessage')}><Sparkles className={cn('h-3.5 w-3.5', generateMessage.isPending && 'animate-pulse')} /></Button><Button type="submit" size="icon" className="h-7 w-7" disabled={!canUseGit || staged.length === 0 || !commitMessage.trim()} aria-label={t('chat:workspace.gitPanel.commitStaged')} title={t('chat:workspace.commit')}><Check className="h-3.5 w-3.5" /></Button></div></div></form> : null}
          </section>
        </div>
      </> : null}

      <WorkspaceGitBranchSheet groupId={groupId} open={branchSheetOpen} onOpenChange={setBranchSheetOpen} onError={setGitError} onSetRemote={() => { setRemoteUrl(status.data?.remote_url ?? ''); setRemoteDialogOpen(true) }} />
      <Dialog open={remoteDialogOpen} onOpenChange={setRemoteDialogOpen}><DialogContent className="w-[calc(100vw-2rem)] sm:max-w-md"><DialogHeader><DialogTitle>{t('chat:workspace.gitPanel.setRemoteTitle')}</DialogTitle><DialogDescription>{t('chat:workspace.gitPanel.setRemoteDescription')}</DialogDescription></DialogHeader><Input value={remoteUrl} onChange={(event) => setRemoteUrl(event.target.value)} placeholder={t('chat:workspace.gitPanel.remoteUrlPlaceholder')} aria-label={t('chat:workspace.gitPanel.remoteUrl')} /><DialogFooter><Button type="button" variant="outline" onClick={() => setRemoteDialogOpen(false)}>{t('common:actions.cancel')}</Button><Button type="button" disabled={!remoteUrl.trim() || setRemote.isPending} onClick={saveRemoteAndRetry}>{t('chat:workspace.gitPanel.saveRetry')}</Button></DialogFooter></DialogContent></Dialog>
      <ConfirmDialog open={discardAllOpen} onOpenChange={setDiscardAllOpen} title={t('chat:workspace.gitPanel.discardAllTitle')} description={t('chat:workspace.gitPanel.discardAllDescription')} confirmLabel={t('chat:workspace.gitPanel.discardAll')} destructive onConfirm={async () => { await discard.mutateAsync({ paths: [], all: true }) }} />
      <ConfirmDialog open={discardTarget !== null} onOpenChange={(open) => { if (!open) setDiscardTarget(null) }} title={t('chat:workspace.gitPanel.discardFileTitle')} description={discardTarget ? t('chat:workspace.gitPanel.discardFileDescription', { path: discardTarget.path }) : undefined} confirmLabel={t('chat:workspace.discard')} destructive onConfirm={async () => { if (discardTarget) await discard.mutateAsync({ paths: [discardTarget.path], all: false }) }} />
    </div>
  )
}
