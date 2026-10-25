import { useState } from 'react'
import { Check, ChevronDown, Pencil, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ThreadStatusIndicator } from '@/components/chat/ConversationStatusDot'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { GroupThread } from '@/types/api'

interface Props {
  groupId: string
  threads: GroupThread[]
  selectedThread: GroupThread | undefined
  disabled: boolean
  onSelect: (threadId: string) => void
  onPrefetch: (threadId: string) => void
  onRename: (thread: GroupThread) => void
  onDelete: (threads: GroupThread[], bulk: boolean) => void
}

export function GroupTaskSwitcher({
  groupId, threads, selectedThread, disabled, onSelect, onPrefetch, onRename, onDelete,
}: Props) {
  const { t } = useTranslation('groups')
  const [open, setOpen] = useState(false)
  const active = threads.filter((thread) => thread.status !== 'archived')
  const archived = threads.filter((thread) => thread.status === 'archived')
  const title = (thread: GroupThread) => thread.title || t('tasks.untitled')
  const label = (thread: GroupThread) => [
    title(thread), thread.git_branch, thread.status === 'archived' ? t('tasks.archived') : null,
  ].filter(Boolean).join(' · ')

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          className="chat-task-switcher h-8 w-32 justify-between gap-1 px-2 font-medium sm:w-48 lg:w-60"
          disabled={disabled || threads.length === 0}
          aria-label={t('tasks.switcher')}
          title={selectedThread ? label(selectedThread) : undefined}
        >
          <span className="flex min-w-0 items-center gap-1.5">
            {selectedThread && selectedThread.status !== 'archived' ? (
              <ThreadStatusIndicator conversationId={groupId} threadId={selectedThread.id} />
            ) : null}
            <span className="truncate">{selectedThread ? label(selectedThread) : t('tasks.none')}</span>
          </span>
          <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      {/* Note: Independent buttons in a popover keep row actions keyboard accessible
          without nesting interactive controls inside Select options. */}
      <PopoverContent
        align="start"
        aria-label={t('tasks.switcher')}
        className="max-h-[min(28rem,var(--radix-popover-content-available-height))] w-80 max-w-[calc(100vw-2rem)] overflow-y-auto"
      >
        {[
          { heading: t('tasks.active'), sectionThreads: active },
          { heading: t('tasks.archived'), sectionThreads: archived },
        ].map(({ heading, sectionThreads }) => {
          if (!sectionThreads.length) return null
          const isArchived = sectionThreads[0].status === 'archived'
          return (
            <div key={heading} role="group" aria-label={heading} className="border-border [&+div]:mt-1 [&+div]:border-t [&+div]:pt-1">
              <div className="flex min-h-8 items-center justify-between gap-2 px-2">
                <span className="text-xs font-semibold text-muted-foreground">{heading}</span>
                {isArchived ? (
                  <Button
                    variant="ghost" size="sm" className="h-7 px-1.5 text-xs text-destructive"
                    disabled={disabled}
                    onClick={() => { setOpen(false); onDelete(archived, true) }}
                  >
                    <Trash2 className="mr-1 h-3.5 w-3.5" aria-hidden="true" />
                    {t('tasks.deleteAllArchived')}
                  </Button>
                ) : null}
              </div>
              {sectionThreads.map((thread) => (
                <div key={thread.id} className="flex min-w-0 items-center gap-0.5 rounded-sm hover:bg-muted/60">
                  <button
                    type="button"
                    className="flex min-h-9 min-w-0 flex-1 items-center gap-1.5 rounded-sm px-2 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                    disabled={disabled}
                    aria-pressed={thread.id === selectedThread?.id}
                    title={label(thread)}
                    onPointerEnter={() => onPrefetch(thread.id)}
                    onFocus={() => onPrefetch(thread.id)}
                    onClick={() => { setOpen(false); onSelect(thread.id) }}
                  >
                    {!isArchived ? <ThreadStatusIndicator conversationId={groupId} threadId={thread.id} /> : null}
                    <span className="min-w-0 flex-1 truncate">{label(thread)}</span>
                    {thread.id === selectedThread?.id ? <Check className="h-4 w-4 shrink-0 text-primary" aria-hidden="true" /> : null}
                  </button>
                  <Button
                    variant="ghost" size="icon" className="h-8 w-8 shrink-0 text-muted-foreground"
                    disabled={disabled} aria-label={t('tasks.renameNamed', { title: title(thread) })}
                    title={t('tasks.rename')}
                    onClick={() => { setOpen(false); onRename(thread) }}
                  ><Pencil className="h-3.5 w-3.5" aria-hidden="true" /></Button>
                  {isArchived ? (
                    <Button
                      variant="ghost" size="icon" className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                      disabled={disabled} aria-label={t('tasks.deleteNamed', { title: title(thread) })}
                      title={t('tasks.delete')}
                      onClick={() => { setOpen(false); onDelete([thread], false) }}
                    ><Trash2 className="h-3.5 w-3.5" aria-hidden="true" /></Button>
                  ) : null}
                </div>
              ))}
            </div>
          )
        })}
      </PopoverContent>
    </Popover>
  )
}
