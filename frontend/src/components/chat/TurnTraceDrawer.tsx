import type { RefObject } from 'react'
import { useState } from 'react'
import { AlertTriangle, Ban, CircleDollarSign, Coins, Footprints, RefreshCcw, Route } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { DispatchDag } from '@/components/chat/DispatchDag'
import { Button } from '@/components/ui/button'
import { AlertDialog, AlertDialogContent, AlertDialogHeader, AlertDialogTitle, AlertDialogDescription, AlertDialogFooter, AlertDialogCancel, AlertDialogAction } from '@/components/ui/alert-dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { useAcknowledgeStoppedTurn, useCancelGroupTurn, useGroupTurnTrace } from '@/hooks/useGroupTurnTrace'
import { cn } from '@/lib/utils'
import { formatNumber } from '@/lib/format'
import { normalizeLanguage } from '@/i18n'
import type { GroupTurnStatus, GroupTurnTerminationReason } from '@/lib/api-v2/types'
import type { GroupAgentRead } from '@/types/api'

interface TurnTraceDrawerProps {
  groupId: string | undefined
  turnId: string | null
  open: boolean
  onOpenChange: (open: boolean) => void
  returnFocusRef?: RefObject<HTMLElement | null>
  agents?: GroupAgentRead[]
}

const activeStatuses = new Set<GroupTurnStatus>(['queued', 'pending', 'running', 'waiting_for_user'])

const turnStatusKeys = {
  queued: 'trace.statuses.queued',
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
  moderator_finished: 'trace.terminationReasons.moderator_finished',
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
      <span className="flex items-center gap-1 text-2xs text-muted-foreground">
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
  agents = [],
}: TurnTraceDrawerProps) {
  const { t, i18n } = useTranslation('chat')
  const acknowledge = useAcknowledgeStoppedTurn()
  const [confirming, setConfirming] = useState(false)
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const trace = useGroupTurnTrace(groupId, turnId)
  const cancelTurn = useCancelGroupTurn()
  const data = trace.data
  const maxHop = data?.dispatches.reduce((maximum, dispatch) => Math.max(maximum, dispatch.hop), 0) ?? 0
  const canCancel = data ? activeStatuses.has(data.turn.status) : false
  const agentName = (agentId: string) =>
    agents.find((agent) => agent.agent_id === agentId)?.display_name ?? agentId

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
              {data.stop_confirmation_required ? (
                <div className="space-y-3 rounded-md border border-amber-500/30 bg-amber-500/5 p-3" role="status">
                  <p className="flex items-center gap-2 text-sm font-medium"><AlertTriangle className="h-4 w-4 shrink-0" aria-hidden="true" />{t('trace.stopConfirmation.title')}</p>
                  <p className="text-xs leading-relaxed text-muted-foreground">{t('trace.stopConfirmation.description')}</p>
                  <Button size="sm" variant="outline" disabled={acknowledge.isPending} onClick={() => setConfirming(true)}>{t('trace.stopConfirmation.open')}</Button>
                  <AlertDialog open={confirming} onOpenChange={setConfirming}>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>{t('trace.stopConfirmation.title')}</AlertDialogTitle>
                        <AlertDialogDescription>{t('trace.stopConfirmation.confirmDescription')}</AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>{t('common:actions.cancel')}</AlertDialogCancel>
                        <AlertDialogAction onClick={() => { if (groupId && turnId) acknowledge.mutate({ groupId, turnId }) }}>{t('trace.stopConfirmation.confirm')}</AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
                </div>
              ) : null}
              {acknowledge.isError ? <p className="text-xs text-destructive" role="alert">{acknowledge.error.message}</p> : null}
              {data.execution_locations?.map((location) => (
                <div key={`${location.agent_id}:${location.computer_id}:${location.generation}`} className="rounded-md border border-border p-3 text-xs">
                  <p className="font-medium">{t('computers:title')} · {location.computer_name}</p>
                  <p className="mt-1 text-muted-foreground">{agentName(location.agent_id)} · {t('computers:generation')} {location.generation}</p>
                  <p className="mt-1 truncate font-mono text-2xs text-muted-foreground" title={location.computer_id}>{location.computer_id}</p>
                </div>
              ))}

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

              <DispatchDag dispatches={data.dispatches} agentName={agentName} />
            </div>
          </ScrollArea>
        ) : null}
      </SheetContent>
    </Sheet>
  )
}
