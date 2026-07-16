import type { ReactNode } from 'react'
import { Brain, ChevronRight, Terminal, Wrench } from 'lucide-react'

import { cn } from '@/lib/utils'

export interface ActivityReasoningSegment {
  id: string
  content: string
  streaming?: boolean
}

export interface ActivityToolItem {
  id: string
  name: string
  status: string | null
  argsSummary?: string | null
  resultSummary?: string | null
  details?: ReactNode
  defaultOpen?: boolean
  kind?: 'tool' | 'external'
}

interface AgentActivityBubbleProps {
  reasoning: ActivityReasoningSegment[]
  tools: ActivityToolItem[]
  active?: boolean
}

function statusLabel(status: string | null): string {
  return (status ?? 'unknown').replace(/_/g, ' ')
}

function statusClasses(status: string | null): string {
  if (status === 'completed') return 'border-primary/30 bg-primary/10 text-primary'
  if (status === 'started' || status === 'running') {
    return 'border-warning bg-warning text-warning-foreground'
  }
  if (status === 'failed' || status === 'cancelled') {
    return 'border-destructive/30 bg-destructive/10 text-destructive'
  }
  if (status === 'input_required' || status === 'approval_required') {
    return 'border-warning bg-warning text-warning-foreground'
  }
  return 'border-border bg-muted text-muted-foreground'
}

function DetailBlock({ label, value }: { label: string; value?: string | null }) {
  if (!value) return null
  return (
    <details className="rounded-md border border-border bg-background">
      <summary className="cursor-pointer px-2 py-1 text-[11px] font-medium text-muted-foreground hover:text-foreground">
        {label}
      </summary>
      <pre className="max-h-36 overflow-auto whitespace-pre-wrap break-words border-t border-border px-2 py-1.5 text-xs leading-snug text-foreground">
        {value}
      </pre>
    </details>
  )
}

function ToolRow({ tool }: { tool: ActivityToolItem }) {
  const hasDetails = Boolean(tool.details || tool.argsSummary || tool.resultSummary)
  const Icon = tool.kind === 'external' ? Terminal : Wrench

  return (
    <details open={tool.defaultOpen} className="group/tool min-w-0">
      <summary className="flex min-w-0 cursor-pointer list-none items-center gap-2 px-2.5 py-2 text-[11px] hover:bg-muted/40 [&::-webkit-details-marker]:hidden">
        <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate font-mono font-semibold text-foreground">
          {tool.name}
        </span>
        <span
          className={cn(
            'shrink-0 rounded-[3px] border px-1.5 py-0.5 text-[10px] font-medium capitalize leading-none',
            statusClasses(tool.status),
          )}
        >
          {statusLabel(tool.status)}
        </span>
        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform group-open/tool:rotate-90" />
      </summary>
      <div className="space-y-1.5 border-t border-border bg-muted/20 px-2.5 py-2">
        {tool.details ?? (
          <>
            <DetailBlock label="Arguments" value={tool.argsSummary} />
            <DetailBlock label="Result" value={tool.resultSummary} />
            {!hasDetails ? (
              <p className="text-xs text-muted-foreground">No details returned.</p>
            ) : null}
          </>
        )}
      </div>
    </details>
  )
}

export function AgentActivityBubble({
  reasoning,
  tools,
  active = false,
}: AgentActivityBubbleProps) {
  const reasoningSegments = reasoning.filter((segment) => segment.content.trim().length > 0)
  if (reasoningSegments.length === 0 && tools.length === 0) return null

  const counts = [
    reasoningSegments.length > 0
      ? `${reasoningSegments.length} reasoning`
      : null,
    tools.length > 0 ? `${tools.length} ${tools.length === 1 ? 'tool' : 'tools'}` : null,
  ].filter((value): value is string => value !== null)
  const accessibleLabel = `Activity: ${counts.join(', ')}${active ? ', active' : ''}`

  return (
    <details
      aria-label={accessibleLabel}
      className="group/activity w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-border bg-muted/20"
    >
      <summary className="flex min-w-0 cursor-pointer list-none items-center gap-2 px-2.5 py-2 text-[11px] text-muted-foreground hover:bg-muted/40 hover:text-foreground [&::-webkit-details-marker]:hidden">
        <Wrench className="h-3.5 w-3.5 shrink-0" />
        <span className="font-medium text-foreground">Activity</span>
        <span className="min-w-0 truncate">{counts.join(' · ')}</span>
        {active ? (
          <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-warning-foreground" />
        ) : null}
        <ChevronRight className="ml-auto h-3.5 w-3.5 shrink-0 transition-transform group-open/activity:rotate-90" />
      </summary>
      <div className="min-w-0 border-t border-border">
        {reasoningSegments.length > 0 ? (
          <section aria-label="Reasoning" className="min-w-0 px-2.5 py-2.5">
            <div className="mb-2 flex items-center gap-1.5 text-[11px] font-medium text-warning-foreground">
              <Brain className="h-3.5 w-3.5" />
              <span>Reasoning</span>
              <span className="font-normal text-muted-foreground">
                {reasoningSegments.length} {reasoningSegments.length === 1 ? 'segment' : 'segments'}
              </span>
            </div>
            <div className="max-h-56 min-w-0 overflow-auto rounded-md border border-dashed border-warning bg-warning/35">
              {reasoningSegments.map((segment, index) => (
                <div
                  key={segment.id}
                  className={cn(
                    'whitespace-pre-wrap break-words px-2.5 py-2 text-xs leading-relaxed text-foreground',
                    index > 0 && 'border-t border-warning/70',
                  )}
                >
                  {segment.content}
                </div>
              ))}
            </div>
          </section>
        ) : null}
        {tools.length > 0 ? (
          <section
            aria-label="Tool calls"
            className={cn('min-w-0', reasoningSegments.length > 0 && 'border-t border-border')}
          >
            <div className="divide-y divide-border">
              {tools.map((tool) => (
                <ToolRow key={tool.id} tool={tool} />
              ))}
            </div>
          </section>
        ) : null}
      </div>
    </details>
  )
}
