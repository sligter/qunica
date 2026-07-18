import { Activity, AlertTriangle, Ban, ChevronRight, CircleCheck, Clock3 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import '@/i18n'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { GroupTurnStatus } from '@/lib/api-v2/types'
import type { SchedulerCriticalSummaryKind } from '@/stores/messageStore'

export interface TurnCriticalSummary {
  id: string
  message: string
  tone?: 'neutral' | 'warning' | 'destructive'
  kind?: SchedulerCriticalSummaryKind
  count?: number
  target_agent_id?: string | null
}

interface TurnSummaryProps {
  turnId: string
  status: GroupTurnStatus
  summaries?: readonly TurnCriticalSummary[]
  onViewTrace: (turnId: string, trigger: HTMLButtonElement) => void
  className?: string
}

const turnStatusKeys = {
  pending: 'trace.statuses.pending',
  running: 'trace.statuses.running',
  waiting_for_user: 'trace.statuses.waiting_for_user',
  completed: 'trace.statuses.completed',
  silence: 'trace.statuses.silence',
  budget_exhausted: 'trace.statuses.budget_exhausted',
  failure_budget_exhausted: 'trace.statuses.failure_budget_exhausted',
  cancelled: 'trace.statuses.cancelled',
  superseded: 'trace.statuses.superseded',
  failed: 'trace.statuses.failed',
} as const satisfies Record<GroupTurnStatus, string>

interface SummaryTranslation {
  key: string
  values?: Record<string, string | number>
}

function knownSummaryTranslation(summary: TurnCriticalSummary): SummaryTranslation | null {
  if (!summary.kind) return null
  switch (summary.kind) {
    case 'deterministic_selection': {
      if (summary.count === undefined) return null
      const expected = `Scheduler selected ${summary.count} ${summary.count === 1 ? 'speaker' : 'speakers'}`
      return summary.message === expected
        ? { key: 'trace.summaries.deterministicSelection', values: { count: summary.count } }
        : null
    }
    case 'call':
      return summary.target_agent_id && summary.message === `Agent call routed to ${summary.target_agent_id}`
        ? { key: 'trace.summaries.call', values: { target: summary.target_agent_id } }
        : null
    case 'handoff':
      return summary.target_agent_id && summary.message === `Handoff routed to ${summary.target_agent_id}`
        ? { key: 'trace.summaries.handoff', values: { target: summary.target_agent_id } }
        : null
    case 'dispatch_failed':
      return summary.target_agent_id && summary.message === `Dispatch to ${summary.target_agent_id} failed`
        ? { key: 'trace.summaries.dispatchFailed', values: { target: summary.target_agent_id } }
        : null
    case 'moderator_fallback':
      return summary.target_agent_id && summary.message === `Moderator fallback selected ${summary.target_agent_id}`
        ? { key: 'trace.summaries.moderatorFallback', values: { target: summary.target_agent_id } }
        : null
    case 'cancelled':
      return summary.message === 'Turn cancelled' ? { key: 'trace.summaries.cancelled' } : null
    case 'superseded':
      return summary.message === 'Turn superseded by a newer message'
        ? { key: 'trace.summaries.superseded' }
        : null
    case 'budget_exhausted':
      if (summary.message === 'Turn reached its budget limit') {
        return { key: 'trace.summaries.budgetExhausted' }
      }
      return summary.message === 'Turn stopped after repeated failures'
        ? { key: 'trace.summaries.failureBudgetExhausted' }
        : null
    case 'waiting_for_user':
      return summary.message === 'Turn is waiting for user input'
        ? { key: 'trace.summaries.waitingForUser' }
        : null
    case 'silence':
      return summary.message === 'Turn completed without a visible reply'
        ? { key: 'trace.summaries.silence' }
        : null
    case 'failed':
      if (summary.message === 'Turn failed while persisting scheduler state') {
        return { key: 'trace.summaries.persistenceFailed' }
      }
      return summary.message === 'Turn failed' ? { key: 'trace.summaries.failed' } : null
  }
}

function statusIcon(status: GroupTurnStatus) {
  if (status === 'completed' || status === 'silence') return CircleCheck
  if (status === 'pending' || status === 'waiting_for_user') return Clock3
  if (status === 'running') return Activity
  if (status === 'cancelled' || status === 'superseded') return Ban
  return AlertTriangle
}

function statusClasses(status: GroupTurnStatus): string {
  if (status === 'completed' || status === 'silence') return 'text-success'
  if (status === 'failed') return 'text-destructive'
  if (status === 'budget_exhausted' || status === 'failure_budget_exhausted') {
    return 'text-warning-foreground'
  }
  return 'text-muted-foreground'
}

export function TurnSummary({
  turnId,
  status,
  summaries = [],
  onViewTrace,
  className,
}: TurnSummaryProps) {
  const { t } = useTranslation('chat')
  const StatusIcon = statusIcon(status)

  return (
    <section
      aria-label={t('trace.summary')}
      className={cn('mx-4 my-1 border-l-2 border-border py-1 pl-3', className)}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <span
          role="status"
          aria-live="polite"
          className={cn('inline-flex min-w-0 items-center gap-1.5 text-xs font-medium', statusClasses(status))}
        >
          <StatusIcon className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>{t(turnStatusKeys[status])}</span>
        </span>
        {summaries.slice(-2).map((summary) => {
          const translation = knownSummaryTranslation(summary)
          const message = translation ? t(translation.key, translation.values) : summary.message
          return (
            <span
              key={summary.id}
              className={cn(
                'max-w-full truncate text-xs text-muted-foreground',
                summary.tone === 'warning' && 'text-warning-foreground',
                summary.tone === 'destructive' && 'text-destructive',
              )}
              title={message}
            >
              {message}
            </span>
          )
        })}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="ml-auto h-7 shrink-0 px-2"
          onClick={(event) => onViewTrace(turnId, event.currentTarget)}
        >
          {t('trace.view')}
          <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
        </Button>
      </div>
    </section>
  )
}
