import { ChevronDown, Circle, CircleCheck, CircleDot, ListTodo } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/lib/utils'
import type { TodoItem, TodoStatus } from '@/types/api'

interface TodoChecklistProps {
  todos: TodoItem[]
  className?: string
  /** Let the reader fold the items away and keep only the progress line. */
  collapsible?: boolean
}

const statusIcons = {
  pending: Circle,
  in_progress: CircleDot,
  completed: CircleCheck,
} as const satisfies Record<TodoStatus, typeof Circle>

const statusItemClasses = {
  pending: 'text-muted-foreground',
  in_progress: 'font-medium text-foreground',
  completed: 'text-muted-foreground line-through decoration-muted-foreground/50',
} as const satisfies Record<TodoStatus, string>

const statusIconClasses = {
  pending: 'text-muted-foreground/70',
  in_progress: 'text-warning-foreground',
  completed: 'text-primary',
} as const satisfies Record<TodoStatus, string>

/**
 * The checklist an agent is working through, as it stands right now.
 *
 * Rendered from the whole list every time rather than as a running log:
 * `TodoWrite` replaces its list on each call, and the reason to show a checklist
 * at all is to answer "where is it up to", which a history of revisions buries.
 * It sits above the composer rather than in a reply, because it is progress the
 * user is meant to read at a glance while the turn runs, not part of the answer.
 */
export function TodoChecklist({ todos, className, collapsible = false }: TodoChecklistProps) {
  const { t } = useTranslation('chat')
  const [collapsed, setCollapsed] = useState(false)
  if (todos.length === 0) return null
  const done = todos.filter((todo) => todo.status === 'completed').length
  const header = (
    <>
      <ListTodo className="h-3.5 w-3.5 shrink-0" />
      <span className="font-medium text-foreground">{t('todos.title')}</span>
      <span>{t('todos.progress', { done, total: todos.length })}</span>
    </>
  )

  return (
    <section
      aria-label={t('todos.title')}
      className={cn(
        'w-fit max-w-full min-w-0 rounded-md border border-border bg-muted/20 px-2.5 py-2',
        className,
      )}
    >
      {collapsible ? (
        <button
          type="button"
          aria-expanded={!collapsed}
          onClick={() => setCollapsed((value) => !value)}
          className="flex w-full items-center gap-1.5 text-2xs text-muted-foreground hover:text-foreground"
        >
          {header}
          <span
            aria-hidden="true"
            className="ml-1 h-1 min-w-8 flex-1 overflow-hidden rounded-full bg-muted"
          >
            <span
              className="block h-full rounded-full bg-primary transition-[width]"
              style={{ width: `${(done / todos.length) * 100}%` }}
            />
          </span>
          <ChevronDown
            className={cn('h-3.5 w-3.5 shrink-0 transition-transform', collapsed && '-rotate-90')}
          />
        </button>
      ) : (
        <div className="flex items-center gap-1.5 text-2xs text-muted-foreground">{header}</div>
      )}
      <ul className={cn('mt-1.5 min-w-0 space-y-1', collapsible && collapsed && 'hidden')}>
        {todos.map((todo, index) => {
          const Icon = statusIcons[todo.status]
          return (
            <li
              key={`${index}-${todo.content}`}
              className={cn(
                'flex min-w-0 items-start gap-1.5 text-xs leading-5',
                statusItemClasses[todo.status],
              )}
            >
              <Icon
                aria-label={t(`todos.statuses.${todo.status}`)}
                className={cn('mt-0.5 h-3.5 w-3.5 shrink-0', statusIconClasses[todo.status])}
              />
              <span className="min-w-0 break-words">{todo.content}</span>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
