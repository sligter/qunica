import { Circle, CircleCheck, CircleDot, ListTodo } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/lib/utils'
import type { TodoItem, TodoStatus } from '@/types/api'

interface TodoChecklistProps {
  todos: TodoItem[]
  className?: string
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
 * It stays outside the collapsed activity disclosure because it is progress the
 * user is meant to read at a glance, not a tool call they may want to inspect.
 */
export function TodoChecklist({ todos, className }: TodoChecklistProps) {
  const { t } = useTranslation('chat')
  if (todos.length === 0) return null
  const done = todos.filter((todo) => todo.status === 'completed').length

  return (
    <section
      aria-label={t('todos.title')}
      className={cn(
        'w-fit max-w-full min-w-0 rounded-md border border-border bg-muted/20 px-2.5 py-2',
        className,
      )}
    >
      <div className="mb-1.5 flex items-center gap-1.5 text-2xs text-muted-foreground">
        <ListTodo className="h-3.5 w-3.5 shrink-0" />
        <span className="font-medium text-foreground">{t('todos.title')}</span>
        <span>{t('todos.progress', { done, total: todos.length })}</span>
      </div>
      <ul className="min-w-0 space-y-1">
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
