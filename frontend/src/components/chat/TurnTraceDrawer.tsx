import type { RefObject } from 'react'
import { AlertTriangle, Ban, CircleDollarSign, Coins, Footprints, RefreshCcw, Route } from 'lucide-react'

import { DispatchDag } from '@/components/chat/DispatchDag'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { useCancelGroupTurn, useGroupTurnTrace } from '@/hooks/useGroupTurnTrace'
import { cn } from '@/lib/utils'
import type { GroupTurnStatus } from '@/lib/api-v2/types'

interface TurnTraceDrawerProps {
  groupId: string | undefined
  turnId: string | null
  open: boolean
  onOpenChange: (open: boolean) => void
  returnFocusRef?: RefObject<HTMLElement | null>
}

const activeStatuses = new Set<GroupTurnStatus>(['pending', 'running', 'waiting_for_user'])

function humanize(value: string): string {
  return value.replace(/_/g, ' ')
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
  const trace = useGroupTurnTrace(groupId, turnId)
  const cancelTurn = useCancelGroupTurn()
  const data = trace.data
  const maxHop = data?.dispatches.reduce((maximum, dispatch) => Math.max(maximum, dispatch.hop), 0) ?? 0
  const canCancel = data ? activeStatuses.has(data.turn.status) : false

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          returnFocusRef?.current?.focus()
        }}
      >
        <SheetHeader className="shrink-0 border-b border-border px-5 py-4 pr-14">
          <SheetTitle>Scheduler trace</SheetTitle>
          <SheetDescription>
            Durable routing and budget details for this turn.
          </SheetDescription>
        </SheetHeader>

        {trace.isLoading ? (
          <div className="flex flex-1 items-center justify-center" role="status">
            <RefreshCcw className="mr-2 h-4 w-4 animate-spin text-muted-foreground" aria-hidden="true" />
            <span className="text-sm text-muted-foreground">Loading trace...</span>
          </div>
        ) : trace.isError ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center" role="alert">
            <AlertTriangle className="h-6 w-6 text-destructive" aria-hidden="true" />
            <div>
              <p className="text-sm font-medium">Trace unavailable</p>
              <p className="mt-1 max-w-sm text-xs text-muted-foreground">{trace.error.message}</p>
            </div>
            <Button type="button" variant="outline" size="sm" onClick={() => void trace.refetch()}>
              <RefreshCcw className="h-3.5 w-3.5" aria-hidden="true" />
              Retry
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
                  {humanize(data.turn.status)}
                </span>
                {data.turn.termination_reason ? <span className="text-xs text-muted-foreground">{humanize(data.turn.termination_reason)}</span> : null}
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
                    {cancelTurn.isPending ? 'Stopping...' : 'Stop turn'}
                  </Button>
                ) : null}
              </div>
              {cancelTurn.isError ? <p className="text-xs text-destructive" role="alert">{cancelTurn.error.message}</p> : null}

              <div className="grid grid-cols-2 gap-x-3 gap-y-4 border-y border-border py-3 sm:grid-cols-4" aria-label="Turn usage">
                <Metric icon={Footprints} label="Steps" value={data.budget.agent_steps.toLocaleString()} />
                <Metric icon={Route} label="Hops" value={maxHop.toLocaleString()} />
                <Metric icon={Coins} label="Tokens" value={data.budget.total_tokens.toLocaleString()} />
                <Metric
                  icon={CircleDollarSign}
                  label="Cost"
                  value={data.estimated_cost ? `${data.estimated_cost.amount} ${data.estimated_cost.currency}` : 'Cost unavailable'}
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
