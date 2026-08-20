import { useState, type FormEvent } from 'react'
import { Archive, ArchiveRestore, ChevronRight, Eraser, ListPlus, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { ThreadStatusIndicator } from '@/components/chat/ConversationStatusDot'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useArchiveGroupThread, useCreateGroupThread, useDeleteGroupThread, useRestoreGroupThread } from '@/hooks/useGroupThreads'
import { useGroupWorkspaceGitBranches } from '@/hooks/useWorkspaceGit'
import { useClearGroupThreadMessages } from '@/hooks/useGroupMessages'
import type { GroupThread } from '@/types/api'

interface GroupChatHeaderActionsProps {
  groupId: string
  threads: GroupThread[]
  selectedThread: GroupThread | undefined
  onSelect: (threadId: string) => void
  onArchived: (threadId: string) => void
  onDeleted: (threadId: string) => void
  disabled?: boolean
}

export function GroupChatHeaderActions({
  groupId,
  threads,
  selectedThread,
  onSelect,
  onArchived,
  onDeleted,
  disabled = false,
}: GroupChatHeaderActionsProps) {
  const { t } = useTranslation(['groups', 'common'])
  const createThread = useCreateGroupThread(groupId)
  const archiveThread = useArchiveGroupThread(groupId)
  const restoreThread = useRestoreGroupThread(groupId)
  const deleteThread = useDeleteGroupThread(groupId)
  const clearThread = useClearGroupThreadMessages(groupId)
  const [createOpen, setCreateOpen] = useState(false)
  const [archiveOpen, setArchiveOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [clearOpen, setClearOpen] = useState(false)
  const [title, setTitle] = useState('')
  const [gitBranch, setGitBranch] = useState('')
  const gitBranches = useGroupWorkspaceGitBranches(createOpen ? groupId : undefined)
  const activeThreads = threads.filter((thread) => thread.status !== 'archived')
  const archivedThreads = threads.filter((thread) => thread.status === 'archived')
  const selectedArchived = selectedThread?.status === 'archived'
  const displayTitle = (thread: GroupThread) => thread.title || t('tasks.untitled')
  const displayLabel = (thread: GroupThread, archived = false) => [
    displayTitle(thread),
    thread.git_branch,
    archived ? t('tasks.archived') : null,
  ].filter(Boolean).join(' · ')
  const boundBranches = new Set(threads.map((thread) => thread.git_branch).filter(Boolean))
  const availableBranches = (gitBranches.data?.branches ?? []).filter(
    (branch) => branch.kind === 'local' && !branch.current && !boundBranches.has(branch.name),
  )
  const mutating = createThread.isPending
    || archiveThread.isPending
    || restoreThread.isPending
    || deleteThread.isPending
    || clearThread.isPending

  const create = async (event: FormEvent) => {
    event.preventDefault()
    const nextTitle = title.trim()
    if (!nextTitle) return
    try {
      const created = await createThread.mutateAsync({
        title: nextTitle,
        git_branch: gitBranch.trim() || null,
      })
      onSelect(created.id)
      setTitle('')
      setGitBranch('')
      setCreateOpen(false)
    } catch {
      // The mutation error is rendered below the field.
    }
  }

  return (
    <div className="flex min-w-0 items-center gap-0.5">
      <ChevronRight
        className="mx-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground/50"
        aria-hidden="true"
      />
      <Select
        value={selectedThread?.id}
        onValueChange={onSelect}
        disabled={mutating || threads.length === 0}
      >
        <SelectTrigger
          className="h-8 w-32 border-0 bg-transparent px-2 text-left font-medium shadow-none transition-colors hover:bg-muted/60 data-[state=open]:bg-muted/60 sm:w-48 lg:w-60"
          aria-label={t('tasks.switcher')}
          title={selectedThread ? displayLabel(selectedThread) : undefined}
        >
          <span className="!flex min-w-0 flex-1 items-center overflow-hidden">
            <SelectValue className="truncate" placeholder={t('tasks.none')} />
          </span>
        </SelectTrigger>
        <SelectContent className="max-w-[calc(100vw-2rem)] sm:w-80">
          {activeThreads.length > 0 ? (
            <SelectGroup>
              <SelectLabel>{t('tasks.active')}</SelectLabel>
              {activeThreads.map((thread) => (
                <SelectItem
                  key={thread.id}
                  value={thread.id}
                  textValue={displayLabel(thread)}
                  className="min-w-0 whitespace-nowrap"
                >
                  {/* The status travels with the item text, so Radix mirrors
                      the selected task's own state into the trigger. */}
                  <span className="flex min-w-0 items-center gap-1.5">
                    <ThreadStatusIndicator conversationId={groupId} threadId={thread.id} />
                    <span
                      className="block max-w-[calc(100vw-6rem)] truncate sm:max-w-64"
                      title={displayLabel(thread)}
                    >
                      {displayLabel(thread)}
                    </span>
                  </span>
                </SelectItem>
              ))}
            </SelectGroup>
          ) : null}
          {activeThreads.length > 0 && archivedThreads.length > 0 ? <SelectSeparator /> : null}
          {archivedThreads.length > 0 ? (
            <SelectGroup>
              <SelectLabel>{t('tasks.archived')}</SelectLabel>
              {archivedThreads.map((thread) => (
                <SelectItem
                  key={thread.id}
                  value={thread.id}
                  textValue={displayLabel(thread, true)}
                  className="min-w-0 whitespace-nowrap"
                >
                  <span
                    className="block max-w-[calc(100vw-6rem)] truncate sm:max-w-64"
                    title={displayLabel(thread, true)}
                  >
                    {displayLabel(thread, true)}
                  </span>
                </SelectItem>
              ))}
            </SelectGroup>
          ) : null}
        </SelectContent>
      </Select>

      <Dialog open={createOpen} onOpenChange={(open) => {
        setCreateOpen(open)
        if (open) {
          createThread.reset()
          setGitBranch('')
        }
      }}>
        <DialogTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground"
            disabled={mutating}
            aria-label={t('actions.newTask')}
            title={t('actions.newTask')}
          >
            <ListPlus className="h-4 w-4" aria-hidden="true" />
          </Button>
        </DialogTrigger>
        <DialogContent closeLabel={t('common:actions.close')} className="sm:max-w-sm">
          <form onSubmit={create} className="space-y-4">
            <DialogHeader>
              <DialogTitle>{t('tasks.createTitle')}</DialogTitle>
              <DialogDescription>{t('tasks.createDescription')}</DialogDescription>
            </DialogHeader>
            <div className="space-y-1.5">
              <Label htmlFor="group-task-title">{t('tasks.title')}</Label>
              <Input
                id="group-task-title"
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                maxLength={80}
                autoFocus
                required
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="group-task-git-branch">{t('tasks.gitBranch')}</Label>
              <Input
                id="group-task-git-branch"
                list="group-task-git-branches"
                value={gitBranch}
                onChange={(event) => setGitBranch(event.target.value)}
                maxLength={255}
                autoComplete="off"
                placeholder={t('tasks.gitBranchPlaceholder')}
                aria-describedby="group-task-git-branch-hint"
              />
              <datalist id="group-task-git-branches">
                {availableBranches.map((branch) => (
                  <option key={branch.full_name} value={branch.name} />
                ))}
              </datalist>
              <p id="group-task-git-branch-hint" className="text-xs text-muted-foreground">
                {gitBranches.isLoading
                  ? t('tasks.gitBranchesLoading')
                  : gitBranches.error
                    ? t('tasks.gitBranchesUnavailable')
                    : t('tasks.gitBranchHint')}
              </p>
            </div>
            {createThread.error ? (
              <p role="alert" className="text-xs text-destructive">
                {t('tasks.createError', { message: String(createThread.error) })}
              </p>
            ) : null}
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">{t('common:actions.cancel')}</Button>
              </DialogClose>
              <Button type="submit" disabled={!title.trim() || createThread.isPending}>
                {createThread.isPending ? t('tasks.creating') : t('actions.newTask')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground"
        disabled={disabled || mutating || !selectedThread}
        onClick={() => setClearOpen(true)}
        aria-label={t('tasks.clearMessages')}
        title={t('tasks.clearMessages')}
      >
        <Eraser className="h-4 w-4" aria-hidden="true" />
      </Button>

      {selectedArchived ? (
        <>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground"
            disabled={mutating}
            onClick={() => {
              if (!selectedThread) return
              void restoreThread.mutateAsync(selectedThread.id)
            }}
            aria-label={t('tasks.restore')}
            title={t('tasks.restore')}
          >
            <ArchiveRestore className="h-4 w-4" aria-hidden="true" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground"
            disabled={mutating}
            onClick={() => setDeleteOpen(true)}
            aria-label={t('tasks.delete')}
            title={t('tasks.delete')}
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
          </Button>
        </>
      ) : (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 text-muted-foreground"
          disabled={disabled || mutating || !selectedThread}
          onClick={() => setArchiveOpen(true)}
          aria-label={t('tasks.archive')}
          title={t('tasks.archive')}
        >
          <Archive className="h-4 w-4" aria-hidden="true" />
        </Button>
      )}
      <ConfirmDialog
        open={clearOpen}
        onOpenChange={setClearOpen}
        title={t('tasks.clearMessagesTitle', { title: selectedThread ? displayTitle(selectedThread) : '' })}
        description={t('tasks.clearMessagesDescription')}
        confirmLabel={t('tasks.clearMessages')}
        destructive
        onConfirm={async () => {
          if (selectedThread) await clearThread.mutateAsync(selectedThread.id)
        }}
      />
      <ConfirmDialog
        open={archiveOpen}
        onOpenChange={setArchiveOpen}
        title={t('tasks.archiveTitle', { title: selectedThread ? displayTitle(selectedThread) : '' })}
        description={t('tasks.archiveDescription')}
        confirmLabel={t('tasks.archive')}
        onConfirm={async () => {
          if (!selectedThread) return
          const archived = await archiveThread.mutateAsync(selectedThread.id)
          onArchived(archived.id)
        }}
      />
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('tasks.deleteTitle', { title: selectedThread ? displayTitle(selectedThread) : '' })}
        description={t('tasks.deleteDescription')}
        confirmLabel={t('tasks.delete')}
        destructive
        onConfirm={async () => {
          if (!selectedThread) return
          const deletedId = selectedThread.id
          await deleteThread.mutateAsync(deletedId)
          onDeleted(deletedId)
        }}
      />
    </div>
  )
}
