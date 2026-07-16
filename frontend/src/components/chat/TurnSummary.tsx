import { Activity, AlertTriangle, Ban, ChevronRight, CircleCheck, Clock3 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { GroupTurnStatus } from '@/lib/api-v2/types'

export interface TurnCriticalSummary {
  id: string
  message: string
  tone?: 'neutral' | 'warning' | 'destructive'
}

interface TurnSummaryProps {
  turnId: string
  status: GroupTurnStatus
  summaries?: readonly TurnCriticalSummary[]
  onViewTrace: (turnId: string, trigger: HTMLButtonElement) => void
  className?: string
}

const statusLabels: Record<GroupTurnStatus, string> = {
  pending: 'Scheduled',
  running: 'Running',
  waiting_for_user: 'Waiting for input',
  completed: 'Completed',
  silence: 'Completed silently',
  budget_exhausted: 'Step budget reached',
  failure_budget_exhausted: 'Failure budget reached',
  cancelled: 'Cancelled',
  superseded: 'Superseded',
  failed: 'Failed',
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
  const StatusIcon = statusIcon(status)

  return (
    <section
      aria-label="Scheduler turn summary"
      className={cn('mx-4 my-1 border-l-2 border-border py-1 pl-3', className)}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <span
          role="status"
          aria-live="polite"
          className={cn('inline-flex min-w-0 items-center gap-1.5 text-xs font-medium', statusClasses(status))}
        >
          <StatusIcon className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>{statusLabels[status]}</span>
        </span>
        {summaries.slice(-2).map((summary) => (
          <span
            key={summary.id}
            className={cn(
              'max-w-full truncate text-xs text-muted-foreground',
              summary.tone === 'warning' && 'text-warning-foreground',
              summary.tone === 'destructive' && 'text-destructive',
            )}
            title={summary.message}
          >
            {summary.message}
          </span>
        ))}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="ml-auto h-7 shrink-0 px-2"
          onClick={(event) => onViewTrace(turnId, event.currentTarget)}
        >
          View trace
          <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
        </Button>
      </div>
    </section>
  )
}
