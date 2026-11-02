/**
 * The task list is intentionally self-contained: its search and filters are
 * navigation aids for this popover, not conversation state. Keeping them here
 * also lets filtering preserve the controlled selection and existing actions.
 */

import { useMemo, useState } from 'react'
import { Check, ChevronDown, Pencil, Search, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useShallow } from 'zustand/react/shallow'

import { ThreadStatusIndicator } from '@/components/chat/ConversationStatusDot'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { selectThreadStatus, useConversationActivityStore } from '@/stores/conversationActivityStore'
import type { GroupThread } from '@/types/api'

type TaskCategory = 'all' | 'main' | 'dispatched' | 'watchdog'
type TaskStatusFilter = 'all' | 'waiting'

const CATEGORY_OPTIONS: readonly TaskCategory[] = ['all', 'main', 'dispatched', 'watchdog']

const CATEGORY_LABEL_KEYS: Record<TaskCategory, string> = {
  all: 'categoryAll',
  main: 'main',
  dispatched: 'dispatched',
  watchdog: 'watchdog',
}

function taskCategory(thread: GroupThread): Exclude<TaskCategory, 'all'> {
  // Note: these are the same server-owned markers shown in each row; the
  // filter must not infer a second meaning from titles or thread status.
  if (thread.watchdog_automation_id) return 'watchdog'
  if (thread.origin_thread_id) return 'dispatched'
  return 'main'
}

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
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState<TaskCategory>('all')
  const [statusFilter, setStatusFilter] = useState<TaskStatusFilter>('all')
  // Note: only runs and pending outcomes affect the waiting filter. Avoiding
  // titles/viewed state here keeps unrelated activity updates out of the list.
  const activityState = useConversationActivityStore(
    useShallow((state) => ({ runs: state.runs, pending: state.pending })),
  )
  const allArchived = useMemo(
    () => threads.filter((thread) => thread.status === 'archived'),
    [threads],
  )
  const statusByThreadId = useMemo(
    () => new Map(
      threads.map((thread) => [
        thread.id,
        selectThreadStatus(
          activityState as Parameters<typeof selectThreadStatus>[0],
          groupId,
          thread.id,
        ),
      ]),
    ),
    [activityState, groupId, threads],
  )
  const { active, archived } = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase()
    const filteredActive: GroupThread[] = []
    const filteredArchived: GroupThread[] = []

    for (const thread of threads) {
      const searchable = [thread.title, thread.git_branch, ...(thread.topics ?? [])]
        .filter(Boolean)
        .join('\n')
        .toLocaleLowerCase()
      if (normalizedQuery && !searchable.includes(normalizedQuery)) continue
      if (category !== 'all' && taskCategory(thread) !== category) continue
      if (statusFilter === 'waiting' && statusByThreadId.get(thread.id) !== 'waiting') continue
      if (thread.status === 'archived') filteredArchived.push(thread)
      else filteredActive.push(thread)
    }

    return { active: filteredActive, archived: filteredArchived }
  }, [category, query, statusByThreadId, statusFilter, threads])
  const hasFilters = query.trim().length > 0 || category !== 'all' || statusFilter !== 'all'
  const title = (thread: GroupThread) => thread.title || t('tasks.untitled')
  const metadata = (thread: GroupThread) => [
    thread.git_branch,
    thread.origin_thread_id ? t('tasks.dispatched') : null,
    thread.watchdog_automation_id ? t('tasks.watchdog') : null,
    thread.origin_thread_id && thread.topics?.length ? t('tasks.following', { topics: thread.topics.join(', ') }) : null,
    thread.status === 'archived' ? t('tasks.archived') : null,
  ].filter(Boolean).join(' · ')
  const label = (thread: GroupThread) => [title(thread), metadata(thread)].filter(Boolean).join(' · ')

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
              <ThreadStatusIndicator conversationId={groupId} threadId={selectedThread.id} showLabel />
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
        className="max-h-[min(32rem,var(--radix-popover-content-available-height))] w-96 max-w-[calc(100vw-2rem)] overflow-y-auto"
      >
        <div className="space-y-2 border-b border-border/60 pb-2">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
            <Input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('tasks.searchPlaceholder')}
              aria-label={t('tasks.search')}
              className="h-8 pl-8 pr-2 text-xs"
            />
          </div>
          <div className="flex gap-1 overflow-x-auto pb-0.5" role="group" aria-label={t('tasks.categoryFilter')}>
            {CATEGORY_OPTIONS.map((option) => (
              <Button
                key={option}
                type="button"
                variant={category === option ? 'secondary' : 'ghost'}
                size="sm"
                className="h-7 shrink-0 px-2 text-[11px]"
                aria-pressed={category === option}
                onClick={() => setCategory(option)}
              >
                {t(`tasks.${CATEGORY_LABEL_KEYS[option]}`)}
              </Button>
            ))}
          </div>
          <div role="group" aria-label={t('tasks.statusFilter')}>
            <Button
              type="button"
              variant={statusFilter === 'waiting' ? 'secondary' : 'ghost'}
              size="sm"
              className="h-7 px-2 text-[11px]"
              aria-pressed={statusFilter === 'waiting'}
              onClick={() => setStatusFilter((current) => current === 'waiting' ? 'all' : 'waiting')}
            >
              {t('tasks.waitingOnly')}
            </Button>
          </div>
        </div>
        {!active.length && !archived.length ? (
          <div role="status" className="space-y-2 px-2 py-5 text-center text-xs text-muted-foreground">
            <p>{hasFilters ? t('tasks.noMatches') : t('tasks.none')}</p>
            {hasFilters ? (
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto px-0 text-xs"
                onClick={() => {
                  setQuery('')
                  setCategory('all')
                  setStatusFilter('all')
                }}
              >
                {t('tasks.clearFilters')}
              </Button>
            ) : null}
          </div>
        ) : [
          { heading: t('tasks.active'), sectionThreads: active, isArchived: false },
          { heading: t('tasks.archived'), sectionThreads: archived, isArchived: true },
        ].map(({ heading, sectionThreads, isArchived }) => {
          if (!sectionThreads.length) return null
          return (
            <div key={heading} role="group" aria-label={heading} className="border-border [&+div]:mt-1 [&+div]:border-t [&+div]:pt-1">
              <div className="flex min-h-8 items-center justify-between gap-2 px-2">
                <span className="text-xs font-semibold text-muted-foreground">{heading}</span>
                {isArchived ? (
                  <Button
                    variant="ghost" size="sm" className="h-7 px-1.5 text-xs text-destructive"
                    disabled={disabled}
                    onClick={() => { setOpen(false); onDelete(allArchived, true) }}
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
                    {!isArchived ? <ThreadStatusIndicator conversationId={groupId} threadId={thread.id} showLabel /> : null}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate">{title(thread)}</span>
                      {metadata(thread) ? (
                        <span className="block truncate text-[10px] leading-4 text-muted-foreground">
                          <span className="sr-only"> · </span>
                          {metadata(thread)}
                        </span>
                      ) : null}
                    </span>
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
