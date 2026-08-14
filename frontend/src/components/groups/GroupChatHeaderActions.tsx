import { useState, type FormEvent } from 'react'
import { Archive, ListPlus, ListTodo } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
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
import { useArchiveGroupThread, useCreateGroupThread } from '@/hooks/useGroupThreads'
import type { GroupThread } from '@/types/api'

interface GroupChatHeaderActionsProps {
  groupId: string
  threads: GroupThread[]
  selectedThread: GroupThread | undefined
  onSelect: (threadId: string) => void
  onArchived: (threadId: string) => void
  disabled?: boolean
}

export function GroupChatHeaderActions({
  groupId,
  threads,
  selectedThread,
  onSelect,
  onArchived,
  disabled = false,
}: GroupChatHeaderActionsProps) {
  const { t } = useTranslation(['groups', 'common'])
  const createThread = useCreateGroupThread(groupId)
  const archiveThread = useArchiveGroupThread(groupId)
  const [createOpen, setCreateOpen] = useState(false)
  const [archiveOpen, setArchiveOpen] = useState(false)
  const [title, setTitle] = useState('')
  const activeThreads = threads.filter((thread) => thread.status !== 'archived')
  const archivedThreads = threads.filter((thread) => thread.status === 'archived')
  const busy = disabled || createThread.isPending || archiveThread.isPending
  const displayTitle = (thread: GroupThread) => thread.title || t('tasks.untitled')

  const create = async (event: FormEvent) => {
    event.preventDefault()
    const nextTitle = title.trim()
    if (!nextTitle) return
    try {
      const created = await createThread.mutateAsync(nextTitle)
      onSelect(created.id)
      setTitle('')
      setCreateOpen(false)
    } catch {
      // The mutation error is rendered below the field.
    }
  }

  return (
    <div className="flex min-w-0 items-center gap-1">
      <Select
        value={selectedThread?.id}
        onValueChange={onSelect}
        disabled={busy || threads.length === 0}
      >
        <SelectTrigger
          className="h-8 w-36 border-0 bg-muted/60 px-2 shadow-none sm:w-52 lg:w-64"
          aria-label={t('tasks.switcher')}
        >
          <span className="flex min-w-0 items-center gap-2">
            <ListTodo className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <SelectValue placeholder={t('tasks.none')} />
          </span>
        </SelectTrigger>
        <SelectContent>
          {activeThreads.length > 0 ? (
            <SelectGroup>
              <SelectLabel>{t('tasks.active')}</SelectLabel>
              {activeThreads.map((thread) => (
                <SelectItem key={thread.id} value={thread.id}>
                  {displayTitle(thread)}
                </SelectItem>
              ))}
            </SelectGroup>
          ) : null}
          {activeThreads.length > 0 && archivedThreads.length > 0 ? <SelectSeparator /> : null}
          {archivedThreads.length > 0 ? (
            <SelectGroup>
              <SelectLabel>{t('tasks.archived')}</SelectLabel>
              {archivedThreads.map((thread) => (
                <SelectItem key={thread.id} value={thread.id}>
                  {displayTitle(thread)} · {t('tasks.archived')}
                </SelectItem>
              ))}
            </SelectGroup>
          ) : null}
        </SelectContent>
      </Select>

      <Dialog open={createOpen} onOpenChange={(open) => {
        setCreateOpen(open)
        if (open) createThread.reset()
      }}>
        <DialogTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground"
            disabled={busy}
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
              {createThread.error ? (
                <p role="alert" className="text-xs text-destructive">
                  {t('tasks.createError', { message: String(createThread.error) })}
                </p>
              ) : null}
            </div>
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
        disabled={busy || !selectedThread || selectedThread.status === 'archived'}
        onClick={() => setArchiveOpen(true)}
        aria-label={t('tasks.archive')}
        title={t('tasks.archive')}
      >
        <Archive className="h-4 w-4" aria-hidden="true" />
      </Button>
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
    </div>
  )
}
