import { Brain, ChevronRight, Wrench } from 'lucide-react'

import { cn } from '@/lib/utils'
import type { MessageToolCall } from '@/types/api'

/**
 * Static (non-streaming) renderers for reasoning blocks and tool cards that were
 * persisted in a message's `content_json`. These mirror the live
 * `StreamTimeline` visuals so a reloaded turn looks the same as it did while
 * streaming, but they read plain persisted data instead of live store events.
 */

function toolStatusLabel(status: string | null): string {
  return (status ?? 'unknown').replace(/_/g, ' ')
}

function toolStatusClasses(status: string | null): string {
  if (status === 'completed') return 'border-primary/30 bg-primary/10 text-primary'
  if (status === 'started') return 'border-warning bg-warning text-warning-foreground'
  if (status === 'failed') return 'border-destructive/30 bg-destructive/10 text-destructive'
  if (status === 'input_required' || status === 'approval_required') {
    return 'border-warning bg-warning text-warning-foreground'
  }
  return 'border-border bg-muted text-muted-foreground'
}

function toolRailClasses(status: string | null): string {
  if (status === 'completed') return 'border-l-primary/60'
  if (status === 'started') return 'border-l-warning-foreground/70'
  if (status === 'failed') return 'border-l-destructive/70'
  if (status === 'input_required' || status === 'approval_required') {
    return 'border-l-warning-foreground/70'
  }
  return 'border-l-muted-foreground/40'
}

function DetailBlock({ label, value }: { label: string; value: string | null }) {
  if (!value) return null
  return (
    <details className="mt-1 rounded-md border border-border bg-background">
      <summary className="cursor-pointer px-2 py-1 text-[11px] font-medium text-muted-foreground hover:text-foreground">
        {label}
      </summary>
      <pre className="max-h-36 overflow-auto whitespace-pre-wrap break-words border-t border-border px-2 py-1.5 text-xs leading-snug text-foreground">
        {value}
      </pre>
    </details>
  )
}

function ReasoningBlock({ content }: { content: string }) {
  return (
    <div className="w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-dashed border-warning bg-warning/55">
      <details className="group/reasoning">
        <summary className="flex cursor-pointer list-none items-center gap-1.5 px-2 py-1 text-[11px] font-medium text-warning-foreground hover:bg-warning [&::-webkit-details-marker]:hidden">
          <Brain className="h-3 w-3" />
          <span>Reasoning</span>
          <span className="ml-auto truncate text-[10px] font-normal text-warning-foreground/70">
            {content ? `${content.length} chars` : 'empty'}
          </span>
          <ChevronRight className="h-3.5 w-3.5 shrink-0 transition-transform group-open/reasoning:rotate-90" />
        </summary>
        <div className="max-h-44 overflow-auto whitespace-pre-wrap break-words border-t border-warning px-2 py-1.5 text-xs leading-snug text-foreground">
          {content || ' '}
        </div>
      </details>
    </div>
  )
}

function ToolCard({ call }: { call: MessageToolCall }) {
  const hasDetails = Boolean(call.args_summary || call.result_summary)
  return (
    <div
      className={cn(
        'w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-l-2 bg-background',
        toolRailClasses(call.status),
      )}
    >
      <details className="group/tool">
        <summary className="flex cursor-pointer list-none items-center gap-1.5 px-2 py-1.5 text-[11px] [&::-webkit-details-marker]:hidden">
          <Wrench className="h-3 w-3 text-muted-foreground" />
          <span className="min-w-0 truncate font-mono text-[11px] font-semibold text-foreground">
            {call.tool_name ?? 'Unknown tool'}
          </span>
          <span
            className={cn(
              'rounded-[3px] border px-1.5 py-0.5 text-[10px] font-medium capitalize leading-none',
              toolStatusClasses(call.status),
            )}
          >
            {toolStatusLabel(call.status)}
          </span>
          <ChevronRight className="ml-auto h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform group-open/tool:rotate-90" />
        </summary>
        <div className="border-t border-border bg-muted/20 px-2 py-1.5">
          {hasDetails ? (
            <>
              <DetailBlock label="Arguments" value={call.args_summary} />
              <DetailBlock label="Result" value={call.result_summary} />
            </>
          ) : (
            <p className="text-xs text-muted-foreground">No details returned.</p>
          )}
        </div>
      </details>
    </div>
  )
}

interface PersistedTurnDetailsProps {
  reasoning?: string[] | null
  toolCalls?: MessageToolCall[] | null
}

/** Render persisted reasoning segments and tool cards, or nothing when empty. */
export function PersistedTurnDetails({ reasoning, toolCalls }: PersistedTurnDetailsProps) {
  const reasoningSegments = (reasoning ?? []).filter((segment) => segment.trim().length > 0)
  const calls = toolCalls ?? []
  if (reasoningSegments.length === 0 && calls.length === 0) return null

  return (
    <div className="flex min-w-0 max-w-full flex-col items-start gap-1.5">
      {reasoningSegments.map((segment, index) => (
        <ReasoningBlock key={`reasoning-${index}`} content={segment} />
      ))}
      {calls.map((call, index) => (
        <ToolCard key={call.tool_call_id ?? `tool-${index}`} call={call} />
      ))}
    </div>
  )
}
