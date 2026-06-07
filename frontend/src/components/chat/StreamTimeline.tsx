import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import {
  Brain,
  ChevronRight,
  GitBranch,
  PauseCircle,
  Terminal,
  Wrench,
  XCircle,
} from 'lucide-react'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import {
  humanInputRequestFromText,
  normalizeHumanInputRequest,
  type HumanInputRequest,
} from '@/lib/humanInput'
import { cn } from '@/lib/utils'
import type {
  StreamExternalRunEvent,
  StreamNoticeEvent,
  StreamReasoningEvent,
  StreamResponseDraftEvent,
  StreamRun,
  StreamTimelineEvent,
  StreamToolEvent,
  ToolActivityStatus,
} from '@/stores/messageStore'
import type { ContextUsage } from '@/types/api'

interface StreamTimelineProps {
  run: StreamRun
  onSubmitHumanInput?: (content: string) => void
}

function timeLabel(value: string): string {
  return new Date(value).toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
  })
}

function toolStatusLabel(status: ToolActivityStatus): string {
  return status.replace(/_/g, ' ')
}

function toolStatusClasses(status: ToolActivityStatus): string {
  if (status === 'completed') return 'border-emerald-200 bg-emerald-50 text-emerald-700'
  if (status === 'started') return 'border-amber-200 bg-amber-50 text-amber-700'
  if (status === 'failed') return 'border-red-200 bg-red-50 text-red-700'
  if (status === 'input_required' || status === 'approval_required') {
    return 'border-amber-200 bg-amber-50 text-amber-700'
  }
  return 'border-border bg-muted text-muted-foreground'
}

function toolRailClasses(status: ToolActivityStatus): string {
  if (status === 'completed') return 'border-l-emerald-400'
  if (status === 'started') return 'border-l-amber-400'
  if (status === 'failed') return 'border-l-red-400'
  if (status === 'input_required' || status === 'approval_required') return 'border-l-amber-400'
  return 'border-l-muted-foreground/40'
}

function toolSummary(status: ToolActivityStatus): string {
  if (status === 'started') return 'generating...'
  if (status === 'completed') return 'completed'
  if (status === 'failed') return 'failed'
  if (status === 'input_required') return 'waiting for input'
  if (status === 'approval_required') return 'approval required'
  if (status === 'setup_required') return 'setup required'
  if (status === 'workspace_required') return 'workspace required'
  return 'unavailable'
}

function externalStatusClasses(status: string | undefined): string {
  if (status === 'completed') return 'border-emerald-200 bg-emerald-50 text-emerald-700'
  if (status === 'running') return 'border-amber-200 bg-amber-50 text-amber-700'
  if (status === 'cancelled' || status === 'timeout') {
    return 'border-amber-200 bg-amber-50 text-amber-700'
  }
  if (status) return 'border-red-200 bg-red-50 text-red-700'
  return 'border-border bg-muted text-muted-foreground'
}

function DetailBlock({
  label,
  value,
  defaultOpen = false,
}: {
  label: string
  value: string | undefined
  defaultOpen?: boolean
}) {
  if (!value) return null
  return (
    <details
      open={defaultOpen}
      className="mt-1 rounded-md border border-border bg-background"
    >
      <summary className="cursor-pointer px-2 py-1 text-[11px] font-medium text-muted-foreground hover:text-foreground">
        {label}
      </summary>
      <pre className="max-h-36 overflow-auto whitespace-pre-wrap break-words border-t border-border px-2 py-1.5 text-xs leading-snug text-foreground">
        {value}
      </pre>
    </details>
  )
}

function StatusBadge({ label, className }: { label: string; className: string }) {
  return (
    <span
      className={cn(
        'rounded-[3px] border px-1.5 py-0.5 text-[10px] font-medium capitalize leading-none',
        className,
      )}
    >
      {label}
    </span>
  )
}

function inputRequestKey(request: HumanInputRequest): string {
  const question = request.question.trim().replace(/\s+/g, ' ')
  const choices = (request.choices ?? []).map((choice) => choice.trim()).join('\u0000')
  return `${question}\u0000${choices}`
}

function shouldRenderInputRequest(
  request: HumanInputRequest,
  renderedInputRequests: Set<string>,
): boolean {
  const key = inputRequestKey(request)
  if (renderedInputRequests.has(key)) return false
  renderedInputRequests.add(key)
  return true
}

function WaitingHint({ label }: { label: string }) {
  return (
    <div className="inline-flex w-fit items-center gap-2 rounded-md border border-amber-200 bg-amber-50/70 px-2.5 py-1.5 text-xs text-amber-700">
      <span className="inline-flex items-center gap-0.5" aria-hidden="true">
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500 [animation-delay:140ms]" />
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500 [animation-delay:280ms]" />
      </span>
      <span>{label}</span>
    </div>
  )
}

/** Collapsible reasoning content. Stays open while streaming, auto-collapses when done. */
function ReasoningBlock({
  event,
  defaultOpen = true,
}: {
  event: StreamReasoningEvent
  defaultOpen?: boolean
}) {
  const streaming = event.status === 'streaming'
  const [open, setOpen] = useState(defaultOpen)
  const wasStreaming = useRef(streaming)

  useEffect(() => {
    if (wasStreaming.current && !streaming) setOpen(false)
    wasStreaming.current = streaming
  }, [streaming])

  return (
    <div
      className={cn(
        'w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-dashed bg-amber-50/45',
        streaming ? 'border-amber-300' : 'border-amber-200/80',
      )}
    >
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-1.5 px-2 py-1 text-[11px] font-medium text-amber-800 hover:bg-amber-100/60"
      >
        <Brain className="h-3 w-3" />
        <span>{streaming ? 'Reasoning...' : 'Reasoning'}</span>
        {streaming && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />}
        <span className="ml-auto truncate text-[10px] font-normal text-amber-700/70">
          {event.content ? `${event.content.length} chars` : 'empty'}
        </span>
        <ChevronRight
          className={cn('h-3.5 w-3.5 shrink-0 transition-transform', open && 'rotate-90')}
        />
      </button>
      {open && (
        <div className="max-h-44 overflow-auto whitespace-pre-wrap break-words border-t border-amber-200/80 px-2 py-1.5 text-xs leading-snug text-amber-950/80">
          {event.content || ' '}
        </div>
      )}
    </div>
  )
}

function ToolCard({
  event,
  onSubmitHumanInput,
  renderedInputRequests,
}: {
  event: StreamToolEvent
  onSubmitHumanInput?: (content: string) => void
  renderedInputRequests: Set<string>
}) {
  const inputRequest = normalizeHumanInputRequest(
    event.input_request,
    event.result_summary,
    event.args_summary,
  )
  const streaming = event.status === 'started'
  const openByDefault =
    streaming ||
    event.status === 'input_required' ||
    event.status === 'approval_required' ||
    event.status === 'failed'
  const hasDetails = Boolean(event.args_summary || event.result_summary)
  const summary = toolSummary(event.status)
  return (
    <div
      className={cn(
        'w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-l-2 bg-background',
        toolRailClasses(event.status),
      )}
    >
      <details open={openByDefault} className="group/tool">
        <summary className="flex cursor-pointer list-none items-center gap-1.5 px-2 py-1.5 text-[11px] [&::-webkit-details-marker]:hidden">
          <Wrench className="h-3 w-3 text-muted-foreground" />
          <span className="min-w-0 truncate font-mono text-[11px] font-semibold text-foreground">
            {event.tool_name}
          </span>
          <StatusBadge
            label={toolStatusLabel(event.status)}
            className={toolStatusClasses(event.status)}
          />
          {event.status !== 'completed' && (
            <span className="ml-auto hidden min-w-fit text-[10px] text-muted-foreground sm:inline">
              {summary}
            </span>
          )}
          <ChevronRight
            className={cn(
              'h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform group-open/tool:rotate-90',
              event.status === 'completed' && 'ml-auto',
            )}
          />
        </summary>
        <div className="border-t border-border bg-muted/20 px-2 py-1.5">
          {inputRequest && shouldRenderInputRequest(inputRequest, renderedInputRequests) ? (
            <HumanInputRequestForm
              compact
              request={inputRequest}
              targetDisplayName={event.display_name}
              onSubmitResponse={onSubmitHumanInput}
            />
          ) : hasDetails ? (
            <>
              <DetailBlock label="Arguments" value={event.args_summary} defaultOpen={streaming} />
              <DetailBlock label="Result" value={event.result_summary} defaultOpen={!streaming} />
            </>
          ) : (
            <p className="text-xs text-muted-foreground">No details returned.</p>
          )}
        </div>
      </details>
    </div>
  )
}

function ExternalCard({ event }: { event: StreamExternalRunEvent }) {
  const adapter = event.adapter ? `External CLI: ${event.adapter}` : 'External CLI'
  const exitCode = event.exit_code === undefined ? '' : `exit ${event.exit_code}`
  const statusLabel = event.status ?? exitCode
  const running = event.status === 'running' || statusLabel === ''
  return (
    <div className="w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-l-2 border-border border-l-sky-400 bg-background">
      <details open={running} className="group/external">
        <summary className="flex cursor-pointer list-none items-center gap-1.5 px-2 py-1.5 text-[11px] [&::-webkit-details-marker]:hidden">
          <Terminal className="h-3 w-3 text-muted-foreground" />
          <span className="min-w-0 truncate font-medium text-foreground">{adapter}</span>
          <StatusBadge
            label={statusLabel || 'running'}
            className={externalStatusClasses(event.status)}
          />
          <ChevronRight className="ml-auto h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform group-open/external:rotate-90" />
        </summary>
        <div className="border-t border-border bg-muted/20 px-2 py-1.5">
          <DetailBlock label="Working directory" value={event.cwd} defaultOpen={running} />
          <DetailBlock label="Summary" value={event.summary} defaultOpen={!running} />
        </div>
      </details>
    </div>
  )
}

function ToolHistoryGroup({
  events,
  renderEvent,
}: {
  events: StreamTimelineEvent[]
  renderEvent: (event: StreamTimelineEvent, options?: { historical?: boolean }) => ReactNode
}) {
  if (events.length === 0) return null
  const callCount = events.filter(
    (event) => event.type === 'tool' || event.type === 'external_run',
  ).length

  return (
    <div className="w-fit max-w-full min-w-0 overflow-hidden rounded-md border border-border bg-muted/20">
      <details className="group/history">
        <summary className="flex cursor-pointer list-none items-center gap-1.5 px-2 py-1.5 text-[11px] text-muted-foreground hover:text-foreground [&::-webkit-details-marker]:hidden">
          <Wrench className="h-3 w-3" />
          <span className="font-medium">Previous tool calls</span>
          <StatusBadge
            label={`${callCount} ${callCount === 1 ? 'call' : 'calls'}`}
            className="border-border bg-background text-muted-foreground normal-case"
          />
          <ChevronRight className="ml-auto h-3.5 w-3.5 shrink-0 transition-transform group-open/history:rotate-90" />
        </summary>
        <div className="flex min-w-0 max-w-full flex-col items-start gap-1.5 border-t border-border px-2 py-1.5">
          {events.map((event) => (
            <div key={event.id} className="max-w-full">
              {renderEvent(event, { historical: true })}
            </div>
          ))}
        </div>
      </details>
    </div>
  )
}

function TextPart({
  event,
  groupId,
  onSubmitHumanInput,
  renderedInputRequests,
}: {
  event: StreamResponseDraftEvent | { content: string; display_name: string }
  groupId: string
  onSubmitHumanInput?: (content: string) => void
  renderedInputRequests: Set<string>
}) {
  const content = event.content
  const inputRequest = humanInputRequestFromText(content)
  if (inputRequest) {
    if (!shouldRenderInputRequest(inputRequest, renderedInputRequests)) return null
    return (
      <HumanInputRequestForm
        request={inputRequest}
        targetDisplayName={event.display_name}
        onSubmitResponse={onSubmitHumanInput}
      />
    )
  }
  return (
    <div className="w-fit max-w-full min-w-0 rounded-md border border-l-4 border-border border-l-primary/60 bg-card/90 px-3 py-2 text-foreground shadow-sm">
      <MarkdownMessage content={content || ' '} groupId={groupId} />
    </div>
  )
}

function NoticeShell({
  tone,
  children,
}: {
  tone: 'amber' | 'red' | 'muted'
  children: ReactNode
}) {
  const classes =
    tone === 'red'
      ? 'border-red-200 border-l-red-400 bg-red-50/70 text-red-700'
      : tone === 'amber'
        ? 'border-amber-200 border-l-amber-400 bg-amber-50/70 text-amber-700'
        : 'border-border border-l-muted-foreground/40 bg-background text-muted-foreground'
  return (
    <div
      className={cn(
        'flex w-fit max-w-full min-w-0 items-center gap-1.5 rounded-md border border-l-4 px-2.5 py-1.5 text-xs shadow-sm',
        classes,
      )}
    >
      {children}
    </div>
  )
}

function AgentNotice({
  event,
  onSubmitHumanInput,
  renderedInputRequests,
}: {
  event: StreamNoticeEvent
  onSubmitHumanInput?: (content: string) => void
  renderedInputRequests: Set<string>
}) {
  if (event.type === 'waiting_for_user') {
    const inputRequest = normalizeHumanInputRequest(event.input_request, event.message)
    if (inputRequest) {
      if (!shouldRenderInputRequest(inputRequest, renderedInputRequests)) return null
      return (
        <HumanInputRequestForm
          compact
          request={inputRequest}
          targetDisplayName={event.display_name}
          onSubmitResponse={onSubmitHumanInput}
        />
      )
    }
    return (
      <NoticeShell tone="amber">
        <PauseCircle className="h-3.5 w-3.5" />
        {event.message}
      </NoticeShell>
    )
  }
  if (event.type === 'agent_error') {
    return (
      <NoticeShell tone="red">
        <XCircle className="h-3.5 w-3.5" />
        {event.message}
      </NoticeShell>
    )
  }
  if (event.type === 'agent_handoff') {
    return (
      <NoticeShell tone="muted">
        <GitBranch className="h-3.5 w-3.5" />
        {event.message}
      </NoticeShell>
    )
  }
  // agent_silent
  return <NoticeShell tone="muted">{event.message}</NoticeShell>
}

interface AgentBlock {
  kind: 'agent'
  agentId: string
  displayName: string
  contextUsage?: ContextUsage | null
  events: StreamTimelineEvent[]
  lastAt: string
}

interface NoticeBlock {
  kind: 'notice'
  event: StreamNoticeEvent
}

type RenderBlock = AgentBlock | NoticeBlock

function eventAgentId(event: StreamTimelineEvent): string | undefined {
  if ('agent_id' in event && event.agent_id) return event.agent_id
  return undefined
}

function eventTime(event: StreamTimelineEvent): string {
  return ('updated_at' in event ? event.updated_at : undefined) ?? event.created_at
}

function eventContextUsage(event: StreamTimelineEvent): ContextUsage | null | undefined {
  if ('context_usage' in event) return event.context_usage
  return undefined
}

function buildBlocks(events: StreamTimelineEvent[]): RenderBlock[] {
  const blocks: RenderBlock[] = []
  for (const event of events) {
    const agentId = eventAgentId(event)
    if (!agentId) {
      // Stream-level notices (done / warning / silence) without an agent owner.
      blocks.push({ kind: 'notice', event: event as StreamNoticeEvent })
      continue
    }
    const displayName = 'display_name' in event ? event.display_name : 'Agent'
    const contextUsage = eventContextUsage(event)
    const last = blocks[blocks.length - 1]
    if (last && last.kind === 'agent' && last.agentId === agentId) {
      last.events.push(event)
      last.lastAt = eventTime(event)
      if (displayName) last.displayName = displayName
      if (contextUsage !== undefined) last.contextUsage = contextUsage
    } else {
      blocks.push({
        kind: 'agent',
        agentId,
        displayName: displayName || 'Agent',
        contextUsage,
        events: [event],
        lastAt: eventTime(event),
      })
    }
  }
  return blocks
}

function blockIsStreaming(block: AgentBlock): boolean {
  return block.events.some(
    (event) =>
      (event.type === 'reasoning' && event.status === 'streaming') ||
      (event.type === 'response_draft' && event.status === 'streaming') ||
      (event.type === 'tool' && event.status === 'started'),
  )
}

function blockIsWaiting(block: AgentBlock): boolean {
  return block.events.every((event) => event.type === 'agent_start')
}

function isToolCallEvent(event: StreamTimelineEvent): boolean {
  return event.type === 'tool' || event.type === 'external_run'
}

function isFoldableToolStatus(status: ToolActivityStatus): boolean {
  return (
    status === 'completed' ||
    status === 'failed' ||
    status === 'unavailable' ||
    status === 'setup_required' ||
    status === 'workspace_required'
  )
}

function isFoldableCallTraceEvent(event: StreamTimelineEvent): boolean {
  if (event.type === 'reasoning') return event.status === 'done'
  if (event.type === 'tool') return isFoldableToolStatus(event.status)
  if (event.type === 'external_run') {
    return (event.status !== undefined && event.status !== 'running') || event.exit_code !== undefined
  }
  return false
}

function latestCallTraceStart(events: StreamTimelineEvent[]): number {
  let latestCallIndex = -1
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (isToolCallEvent(events[index]!)) {
      latestCallIndex = index
      break
    }
  }
  if (latestCallIndex === -1) return -1

  let start = latestCallIndex
  for (let index = latestCallIndex - 1; index >= 0; index -= 1) {
    const event = events[index]!
    if (event.type !== 'reasoning') break
    start = index
  }
  return start
}

interface ToolHistoryRenderItem {
  kind: 'tool_history'
  id: string
  events: StreamTimelineEvent[]
}

type TimelineRenderItem = StreamTimelineEvent | ToolHistoryRenderItem

function compactToolHistory(events: StreamTimelineEvent[]): TimelineRenderItem[] {
  const latestTraceStart = latestCallTraceStart(events)
  if (latestTraceStart <= 0) return events

  const items: TimelineRenderItem[] = []
  const historyEvents: StreamTimelineEvent[] = []
  let insertedHistory = false

  for (let index = 0; index < events.length; index += 1) {
    const event = events[index]!
    if (index < latestTraceStart && isFoldableCallTraceEvent(event)) {
      historyEvents.push(event)
      continue
    }

    if (!insertedHistory && historyEvents.length > 0) {
      items.push({
        kind: 'tool_history',
        id: `tool-history-${historyEvents[0]!.id}-${historyEvents.length}`,
        events: historyEvents,
      })
      insertedHistory = true
    }
    items.push(event)
  }

  return items
}

function isToolHistoryRenderItem(item: TimelineRenderItem): item is ToolHistoryRenderItem {
  return 'kind' in item && item.kind === 'tool_history'
}

function AgentBlockView({
  block,
  runStatus,
  groupId,
  fallbackUsage,
  onSubmitHumanInput,
  renderedInputRequests,
}: {
  block: AgentBlock
  runStatus: StreamRun['status']
  groupId: string
  fallbackUsage: ContextUsage | null
  onSubmitHumanInput?: (content: string) => void
  renderedInputRequests: Set<string>
}) {
  const waiting = runStatus === 'active' && blockIsWaiting(block)
  const streaming = blockIsStreaming(block) || waiting
  const renderEvent = (
    event: StreamTimelineEvent,
    options?: { historical?: boolean },
  ): ReactNode => {
    if (event.type === 'agent_start') return null
    if (event.type === 'reasoning') {
      return <ReasoningBlock key={event.id} event={event} defaultOpen={!options?.historical} />
    }
    if (event.type === 'response_draft' || event.type === 'agent_message') {
      return (
        <TextPart
          key={event.id}
          event={event}
          groupId={groupId}
          onSubmitHumanInput={onSubmitHumanInput}
          renderedInputRequests={renderedInputRequests}
        />
      )
    }
    if (event.type === 'tool') {
      return (
        <ToolCard
          key={event.id}
          event={event}
          onSubmitHumanInput={onSubmitHumanInput}
          renderedInputRequests={renderedInputRequests}
        />
      )
    }
    if (event.type === 'external_run') {
      return <ExternalCard key={event.id} event={event} />
    }
    if (event.type === 'done') return null
    return (
      <AgentNotice
        key={event.id}
        event={event}
        onSubmitHumanInput={onSubmitHumanInput}
        renderedInputRequests={renderedInputRequests}
      />
    )
  }
  const renderItems = compactToolHistory(block.events)

  return (
    <div className="flex w-full gap-3 px-4 py-1.5">
      <AgentAvatar
        name={block.displayName}
        className="mt-0.5"
        contextUsage={block.contextUsage ?? fallbackUsage}
      />
      <div className="flex min-w-0 max-w-[88%] flex-col gap-1 md:max-w-[82%]">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{block.displayName}</span>
          {streaming ? (
            <span className="inline-flex items-center gap-1 rounded-[3px] border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[10px] text-amber-700">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />
              streaming
            </span>
          ) : (
            <span>{timeLabel(block.lastAt)}</span>
          )}
        </div>
        <div className="flex min-w-0 max-w-full flex-col items-start gap-1.5">
          {waiting ? <WaitingHint label="Preparing response..." /> : null}
          {renderItems.map((item) => {
            if (isToolHistoryRenderItem(item)) {
              return (
                <ToolHistoryGroup
                  key={item.id}
                  events={item.events}
                  renderEvent={renderEvent}
                />
              )
            }
            return renderEvent(item)
          })}
        </div>
      </div>
    </div>
  )
}

export function StreamTimeline({ run, onSubmitHumanInput }: StreamTimelineProps) {
  const groupAgents = useGroupAgents(run.group_id)
  // Last-known usage per group-agent. The live `agent_start` events don't carry
  // context_usage (it's computed only after the LLM responds), so without this
  // fallback the avatar ring stays blank for the whole streaming turn.
  const usageByAgentId = useMemo(() => {
    const map = new Map<string, ContextUsage>()
    for (const agent of groupAgents.data ?? []) {
      if (agent.context_usage) map.set(agent.agent_id, agent.context_usage)
    }
    return map
  }, [groupAgents.data])
  const blocks = buildBlocks(run.events)
  const renderedInputRequests = new Set<string>()
  if (blocks.length === 0 && run.status === 'active') {
    return (
      <div className="flex w-full gap-3 px-4 py-2.5">
        <AgentAvatar name="Assistant" className="mt-0.5" />
        <div className="flex min-w-0 max-w-[88%] flex-col gap-1.5 md:max-w-[82%]">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span className="font-medium text-foreground">Assistant</span>
            <span className="inline-flex items-center gap-1 rounded-[3px] border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[10px] text-amber-700">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />
              starting
            </span>
          </div>
          <WaitingHint label="Waiting for agents to start..." />
        </div>
      </div>
    )
  }
  return (
    <div className="flex w-full flex-col">
      {blocks.map((block, index) => {
        if (block.kind === 'notice') {
          if (block.event.type === 'done') return null
          return (
            <div
              key={`${block.event.id}-${index}`}
              className="px-4 py-1 text-center text-xs text-muted-foreground"
            >
              {block.event.message}
            </div>
          )
        }
        return (
          <AgentBlockView
            key={`${block.agentId}-${index}`}
            block={block}
            runStatus={run.status}
            groupId={run.group_id}
            fallbackUsage={usageByAgentId.get(block.agentId) ?? null}
            onSubmitHumanInput={onSubmitHumanInput}
            renderedInputRequests={renderedInputRequests}
          />
        )
      })}
    </div>
  )
}
