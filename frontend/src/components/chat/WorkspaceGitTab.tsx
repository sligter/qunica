import { useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  Check,
  GitBranch,
  Minus,
  Plus,
  RefreshCw,
  Sparkles,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  useCommitGroupWorkspaceGit,
  useGenerateGroupWorkspaceGitCommitMessage,
  useGroupWorkspaceGitStatus,
  usePullGroupWorkspaceGit,
  usePushGroupWorkspaceGit,
  useStageGroupWorkspaceGit,
  useUnstageGroupWorkspaceGit,
} from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { GroupWorkspaceGitFileStatus, GroupWorkspaceGitStatus } from '@/types/api'

interface WorkspaceGitTabProps {
  groupId: string | undefined
}

function displayError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function gitStatusLabel(file: GroupWorkspaceGitFileStatus) {
  if (file.status === '??') return 'untracked'
  const labels: string[] = []
  if (file.staged) labels.push(`staged ${file.status[0].trim() || '?'}`)
  if (file.unstaged) labels.push(`worktree ${file.status[1].trim() || '?'}`)
  return labels.join(', ') || file.status
}

function gitSummary(status: GroupWorkspaceGitStatus | undefined, changed: number) {
  if (status?.available !== true) return 'Workspace Git'
  const parts: string[] = []
  if (status.state === 'conflict') parts.push('Conflicts')
  if (status.state === 'detached') parts.push('Detached HEAD')
  if (status.state === 'initial') parts.push('No commits yet')
  if (status.ahead) parts.push(`${status.ahead} ahead`)
  if (status.behind) parts.push(`${status.behind} behind`)
  if (parts.length > 0) return parts.join(' · ')
  return status.clean ? 'Clean workspace' : `${changed} changed`
}

export function WorkspaceGitTab({ groupId }: WorkspaceGitTabProps) {
  const [gitError, setGitError] = useState<string | null>(null)
  const [commitMessage, setCommitMessage] = useState('')
  const gitStatus = useGroupWorkspaceGitStatus(groupId)
  const gitStage = useStageGroupWorkspaceGit(groupId)
  const gitUnstage = useUnstageGroupWorkspaceGit(groupId)
  const gitCommit = useCommitGroupWorkspaceGit(groupId)
  const gitGenerateCommitMessage = useGenerateGroupWorkspaceGitCommitMessage(groupId)
  const gitPull = usePullGroupWorkspaceGit(groupId)
  const gitPush = usePushGroupWorkspaceGit(groupId)
  const openFile = useFileNavStore((s) => s.openFile)
  const hasGroupId = groupId !== undefined && groupId.length > 0

  const gitFiles = gitStatus.data?.files ?? []
  const isGitBusy =
    gitStage.isPending ||
    gitUnstage.isPending ||
    gitCommit.isPending ||
    gitGenerateCommitMessage.isPending ||
    gitPull.isPending ||
    gitPush.isPending
  const canUseGit = hasGroupId && gitStatus.data?.available === true && !isGitBusy

  const runGit = (operation: Promise<unknown>, clearCommit = false) => {
    setGitError(null)
    void operation
      .then(() => {
        if (clearCommit) setCommitMessage('')
      })
      .catch((error: unknown) => setGitError(displayError(error)))
  }

  const generateCommitMessage = () => {
    setGitError(null)
    void gitGenerateCommitMessage
      .mutateAsync()
      .then((result) => setCommitMessage(result.message))
      .catch((error: unknown) => setGitError(displayError(error)))
  }

  const openChangedFile = (file: GroupWorkspaceGitFileStatus) => {
    if (!hasGroupId) return
    openFile(groupId, file.path)
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <GitBranch className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <p className="truncate text-xs font-medium">{gitStatus.data?.branch ?? 'Git'}</p>
            <p className="truncate text-[10px] text-muted-foreground">
              {gitStatus.data?.available === true
                ? gitSummary(gitStatus.data, gitFiles.length)
                : 'Workspace Git'}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0"
            onClick={() => runGit(gitPull.mutateAsync({}))}
            disabled={!canUseGit}
            aria-label="Pull Git changes"
            title="Pull Git changes"
          >
            <ArrowDown className="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0"
            onClick={() => runGit(gitPush.mutateAsync({}))}
            disabled={!canUseGit}
            aria-label="Push Git changes"
            title="Push Git changes"
          >
            <ArrowUp className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0"
            onClick={() => void gitStatus.refetch()}
            disabled={gitStatus.isFetching || !hasGroupId}
            aria-label="Refresh Git status"
            title="Refresh Git status"
          >
            <RefreshCw className={cn('h-3.5 w-3.5', gitStatus.isFetching && 'animate-spin')} />
          </Button>
        </div>
      </div>

      {gitError && (
        <div className="m-3 shrink-0 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          {gitError}
        </div>
      )}
      {gitStatus.error && (
        <div className="m-3 shrink-0 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          {displayError(gitStatus.error)}
        </div>
      )}

      {!hasGroupId && (
        <p className="p-3 text-sm text-muted-foreground">Select a group to view Git status.</p>
      )}
      {hasGroupId && gitStatus.isLoading && (
        <p className="p-3 text-sm text-muted-foreground">Loading Git status...</p>
      )}
      {gitStatus.data?.available === false && (
        <div className="m-3 rounded-md border border-border bg-muted/50 p-3 text-xs text-muted-foreground">
          {gitStatus.data.message ?? 'This workspace is not a Git repository.'}
        </div>
      )}

      {gitStatus.data?.available === true && (
        <>
          <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
            <span className="text-xs font-medium text-muted-foreground">
              Changes ({gitFiles.length})
            </span>
            <div className="flex items-center gap-1">
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-7 w-7 shrink-0"
                onClick={() => runGit(gitStage.mutateAsync({ paths: [] }))}
                disabled={!canUseGit || gitFiles.length === 0}
                aria-label="Stage all changes"
                title="Stage all changes"
              >
                <Plus className="h-3.5 w-3.5" />
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-7 w-7 shrink-0"
                onClick={() => runGit(gitUnstage.mutateAsync({ paths: [] }))}
                disabled={!canUseGit || !gitFiles.some((file) => file.staged)}
                aria-label="Unstage all changes"
                title="Unstage all changes"
              >
                <Minus className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto">
            {gitFiles.length === 0 && (
              <p className="p-3 text-sm text-muted-foreground">No changes in the workspace.</p>
            )}
            {gitFiles.length > 0 && (
              <ul className="divide-y divide-border">
                {gitFiles.map((file) => (
                  <li
                    key={`${file.status}:${file.path}`}
                    className="group flex items-center gap-2 px-3 py-1.5 hover:bg-muted/70"
                  >
                    <span className="w-6 shrink-0 font-mono text-[11px] text-muted-foreground">
                      {file.status}
                    </span>
                    <button
                      type="button"
                      className="min-w-0 flex-1 truncate text-left text-xs hover:underline"
                      title={file.path}
                      onClick={() => openChangedFile(file)}
                    >
                      {file.path}
                    </button>
                    <span className="sr-only">{gitStatusLabel(file)}</span>
                    {file.unstaged && (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0"
                        onClick={() => runGit(gitStage.mutateAsync({ paths: [file.path] }))}
                        disabled={!canUseGit}
                        aria-label={`Stage ${file.path}`}
                        title={`Stage ${file.path}`}
                      >
                        <Plus className="h-3 w-3" />
                      </Button>
                    )}
                    {file.staged && (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0"
                        onClick={() => runGit(gitUnstage.mutateAsync({ paths: [file.path] }))}
                        disabled={!canUseGit}
                        aria-label={`Unstage ${file.path}`}
                        title={`Unstage ${file.path}`}
                      >
                        <Minus className="h-3 w-3" />
                      </Button>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>

          <form
            className="flex shrink-0 items-center gap-1 border-t border-border p-3"
            onSubmit={(event) => {
              event.preventDefault()
              runGit(gitCommit.mutateAsync({ message: commitMessage.trim() }), true)
            }}
          >
            <Input
              value={commitMessage}
              onChange={(event) => setCommitMessage(event.target.value)}
              placeholder="Commit message"
              className="h-8 min-w-0 text-xs"
              disabled={!canUseGit}
              aria-label="Commit message"
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              className="h-8 w-8 shrink-0"
              onClick={generateCommitMessage}
              disabled={!canUseGit}
              aria-label="Generate commit message"
              title="Generate commit message"
            >
              <Sparkles
                className={cn('h-3.5 w-3.5', gitGenerateCommitMessage.isPending && 'animate-pulse')}
              />
            </Button>
            <Button
              type="submit"
              size="icon"
              className="h-8 w-8 shrink-0"
              disabled={!canUseGit || !commitMessage.trim()}
              aria-label="Commit staged changes"
              title="Commit staged changes"
            >
              <Check className="h-3.5 w-3.5" />
            </Button>
          </form>
        </>
      )}
    </div>
  )
}
