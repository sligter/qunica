import type { RefObject } from 'react'
import { AlertTriangle, Ban, CircleDollarSign, Coins, Footprints, RefreshCcw, Route } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { DispatchDag } from '@/components/chat/DispatchDag'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { useCancelGroupTurn, useGroupTurnTrace } from '@/hooks/useGroupTurnTrace'
import { cn } from '@/lib/utils'
import { formatNumber } from '@/lib/format'
import { normalizeLanguage } from '@/i18n'
import type { GroupTurnStatus, GroupTurnTerminationReason } from '@/lib/api-v2/types'

interface TurnTraceDrawerProps {
  groupId: string | undefined
  turnId: string | null
  open: boolean
  onOpenChange: (open: boolean) => void
  returnFocusRef?: RefObject<HTMLElement | null>
}

const activeStatuses = new Set<GroupTurnStatus>(['pending', 'running', 'waiting_for_user'])

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

const terminationReasonKeys = {
  waiting_for_user: 'trace.terminationReasons.waiting_for_user',
  budget_exhausted: 'trace.terminationReasons.budget_exhausted',
  failure_budget_exhausted: 'trace.terminationReasons.failure_budget_exhausted',
  user_cancelled: 'trace.terminationReasons.user_cancelled',
  superseded: 'trace.terminationReasons.superseded',
  server_restart: 'trace.terminationReasons.server_restart',
  persistence_failed: 'trace.terminationReasons.persistence_failed',
  silence: 'trace.terminationReasons.silence',
} as const satisfies Record<GroupTurnTerminationReason, string>

function isKnownTerminationReason(value: string): value is GroupTurnTerminationReason {
  return Object.prototype.hasOwnProperty.call(terminationReasonKeys, value)
}

function formatCostAmount(amount: string, language: 'en-US' | 'zh-CN'): string {
  const numericAmount = Number(amount)
  return Number.isFinite(numericAmount) ? formatNumber(numericAmount, language) : amount
}

function Metric({ icon: Icon, label, value }: { icon: typeof Footprints; label: string; value: string }) {
  return (
    <div className="min-w-0 border-l border-border pl-3 first:border-l-0 first:pl-0">
      <span className="flex items-center gap-1 text-[11px] text-muted-foreground">
        <Icon className="h-3 w-3" aria-hidden="true" />
        {label}
      </span>
      <strong className="mt-0.5 block truncate text-sm font-semibold">{value}</strong>
    </div>
  )
}

export function TurnTraceDrawer({
  groupId,
  turnId,
  open,
  onOpenChange,
  returnFocusRef,
}: TurnTraceDrawerProps) {
  const { t, i18n } = useTranslation('chat')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const trace = useGroupTurnTrace(groupId, turnId)
  const cancelTurn = useCancelGroupTurn()
  const data = trace.data
  const maxHop = data?.dispatches.reduce((maximum, dispatch) => Math.max(maximum, dispatch.hop), 0) ?? 0
  const canCancel = data ? activeStatuses.has(data.turn.status) : false

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        closeLabel={t('common:actions.close')}
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          returnFocusRef?.current?.focus()
        }}
      >
        <SheetHeader className="shrink-0 border-b border-border px-5 py-4 pr-14">
          <SheetTitle>{t('trace.title')}</SheetTitle>
          <SheetDescription>
            {t('trace.description')}
          </SheetDescription>
        </SheetHeader>

        {trace.isLoading ? (
          <div className="flex flex-1 items-center justify-center" role="status">
            <RefreshCcw className="mr-2 h-4 w-4 animate-spin text-muted-foreground" aria-hidden="true" />
            <span className="text-sm text-muted-foreground">{t('trace.loading')}</span>
          </div>
        ) : trace.isError ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center" role="alert">
            <AlertTriangle className="h-6 w-6 text-destructive" aria-hidden="true" />
            <div>
              <p className="text-sm font-medium">{t('trace.unavailable')}</p>
              <p className="mt-1 max-w-sm text-xs text-muted-foreground">{trace.error.message}</p>
            </div>
            <Button type="button" variant="outline" size="sm" onClick={() => void trace.refetch()}>
              <RefreshCcw className="h-3.5 w-3.5" aria-hidden="true" />
              {t('trace.retry')}
            </Button>
          </div>
        ) : data ? (
          <ScrollArea className="min-h-0 flex-1">
            <div className="space-y-5 px-5 py-4">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <span className={cn(
                  'rounded-md border border-border bg-muted px-2 py-1 text-xs font-medium capitalize',
                  data.turn.status === 'failed' && 'border-destructive/30 bg-destructive/10 text-destructive',
                )}>
                  {t(turnStatusKeys[data.turn.status])}
                </span>
                {data.turn.termination_reason ? (
                  <span className="text-xs text-muted-foreground">
                    {isKnownTerminationReason(data.turn.termination_reason)
                      ? t(terminationReasonKeys[data.turn.termination_reason])
                      : t('common:wireLabels.unknownSelectionReason', {
                          value: data.turn.termination_reason,
                        })}
                  </span>
                ) : null}
                {canCancel ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="ml-auto"
                    disabled={cancelTurn.isPending}
                    onClick={() => {
                      if (groupId && turnId) cancelTurn.mutate({ groupId, turnId })
                    }}
                  >
                    <Ban className="h-3.5 w-3.5" aria-hidden="true" />
                    {cancelTurn.isPending ? t('trace.stopping') : t('trace.stop')}
                  </Button>
                ) : null}
              </div>
              {cancelTurn.isError ? <p className="text-xs text-destructive" role="alert">{cancelTurn.error.message}</p> : null}

              <div className="grid grid-cols-2 gap-x-3 gap-y-4 border-y border-border py-3 sm:grid-cols-4" aria-label={t('trace.usage')}>
                <Metric icon={Footprints} label={t('trace.steps')} value={formatNumber(data.budget.agent_steps, language)} />
                <Metric icon={Route} label={t('trace.hops')} value={formatNumber(maxHop, language)} />
                <Metric icon={Coins} label={t('trace.tokens')} value={formatNumber(data.budget.total_tokens, language)} />
                <Metric
                  icon={CircleDollarSign}
                  label={t('trace.cost')}
                  value={data.estimated_cost
                    ? `${formatCostAmount(data.estimated_cost.amount, language)} ${data.estimated_cost.currency}`
                    : t('trace.costUnavailable')}
                />
              </div>

              <DispatchDag dispatches={data.dispatches} />
            </div>
          </ScrollArea>
        ) : null}
      </SheetContent>
    </Sheet>
  )
}
