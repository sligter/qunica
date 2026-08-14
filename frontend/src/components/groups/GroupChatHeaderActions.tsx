import { useState, type FormEvent } from 'react'
import { Archive, ChevronRight, ListPlus } from 'lucide-react'
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
    <div className="flex min-w-0 items-center gap-0.5">
      <ChevronRight
        className="mx-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground/50"
        aria-hidden="true"
      />
      <Select
        value={selectedThread?.id}
        onValueChange={onSelect}
        disabled={busy || threads.length === 0}
      >
        <SelectTrigger
          className="h-8 w-32 border-0 bg-transparent px-2 text-left font-medium shadow-none transition-colors hover:bg-muted/60 data-[state=open]:bg-muted/60 sm:w-48 lg:w-60"
          aria-label={t('tasks.switcher')}
          title={selectedThread ? displayTitle(selectedThread) : undefined}
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
                  textValue={displayTitle(thread)}
                  className="min-w-0 whitespace-nowrap"
                >
                  <span
                    className="block max-w-[calc(100vw-6rem)] truncate sm:max-w-64"
                    title={displayTitle(thread)}
                  >
                    {displayTitle(thread)}
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
                  textValue={`${displayTitle(thread)} · ${t('tasks.archived')}`}
                  className="min-w-0 whitespace-nowrap"
                >
                  <span
                    className="block max-w-[calc(100vw-6rem)] truncate sm:max-w-64"
                    title={`${displayTitle(thread)} · ${t('tasks.archived')}`}
                  >
                    {displayTitle(thread)} · {t('tasks.archived')}
                  </span>
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
