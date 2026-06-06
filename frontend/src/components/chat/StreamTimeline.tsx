import { useEffect, useRef, useState } from 'react'
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
  if (status === 'started') return 'border-blue-200 bg-blue-50 text-blue-700'
  if (status === 'failed') return 'border-red-200 bg-red-50 text-red-700'
  if (status === 'input_required' || status === 'approval_required') {
    return 'border-amber-200 bg-amber-50 text-amber-700'
  }
  return 'border-border bg-muted text-muted-foreground'
}

function externalStatusClasses(status: string | undefined): string {
  if (status === 'completed') return 'border-emerald-200 bg-emerald-50 text-emerald-700'
  if (status === 'running') return 'border-blue-200 bg-blue-50 text-blue-700'
  if (status === 'cancelled' || status === 'timeout') {
    return 'border-amber-200 bg-amber-50 text-amber-700'
  }
  if (status) return 'border-red-200 bg-red-50 text-red-700'
  return 'border-border bg-muted text-muted-foreground'
}

function DetailBlock({ label, value }: { label: string; value: string | undefined }) {
  if (!value) return null
  return (
    <details className="mt-1.5 rounded-md border border-border bg-background">
      <summary className="cursor-pointer px-2.5 py-1.5 text-[11px] font-medium text-muted-foreground hover:text-foreground">
        {label}
      </summary>
      <pre className="max-h-44 overflow-auto whitespace-pre-wrap break-words border-t border-border px-2.5 py-2 text-xs leading-relaxed text-foreground">
        {value}
      </pre>
    </details>
  )
}

function StatusBadge({ label, className }: { label: string; className: string }) {
  return (
    <span
      className={cn(
        'rounded-full border px-2 py-0.5 text-[10px] font-medium capitalize',
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

/** Collapsible chain-of-thought. Stays open while streaming, auto-collapses when done. */
function ReasoningBlock({ event }: { event: StreamReasoningEvent }) {
  const streaming = event.status === 'streaming'
  const [open, setOpen] = useState(true)
  const wasStreaming = useRef(streaming)

  useEffect(() => {
    if (wasStreaming.current && !streaming) setOpen(false)
    wasStreaming.current = streaming
  }, [streaming])

  return (
    <div className="min-w-0 rounded-md border border-dashed border-border bg-muted/40">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-[11px] font-medium text-muted-foreground hover:text-foreground"
      >
        <Brain className="h-3.5 w-3.5" />
        <span>{streaming ? 'Thinking…' : 'Thinking'}</span>
        {streaming && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />}
        <ChevronRight
          className={cn('ml-auto h-3.5 w-3.5 transition-transform', open && 'rotate-90')}
        />
      </button>
      {open && (
        <div className="max-h-60 overflow-auto whitespace-pre-wrap break-words border-t border-border px-2.5 py-2 text-xs leading-relaxed text-muted-foreground">
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
  return (
    <div className="min-w-0 rounded-md border border-border bg-background px-2.5 py-2">
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs">
        <Wrench className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="font-medium text-foreground">{event.tool_name}</span>
        <StatusBadge
          label={toolStatusLabel(event.status)}
          className={toolStatusClasses(event.status)}
        />
      </div>
      {inputRequest && shouldRenderInputRequest(inputRequest, renderedInputRequests) ? (
        <HumanInputRequestForm
          className="mt-2"
          compact
          request={inputRequest}
          targetDisplayName={event.display_name}
          onSubmitResponse={onSubmitHumanInput}
        />
      ) : (
        <>
          <DetailBlock label="Arguments" value={event.args_summary} />
          <DetailBlock label="Result" value={event.result_summary} />
        </>
      )}
    </div>
  )
}

function ExternalCard({ event }: { event: StreamExternalRunEvent }) {
  const adapter = event.adapter ? `External CLI: ${event.adapter}` : 'External CLI'
  const exitCode = event.exit_code === undefined ? '' : `exit ${event.exit_code}`
  const statusLabel = event.status ?? exitCode
  return (
    <div className="min-w-0 rounded-md border border-border bg-background px-2.5 py-2">
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs">
        <Terminal className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="font-medium text-foreground">{adapter}</span>
        <StatusBadge
          label={statusLabel || 'running'}
          className={externalStatusClasses(event.status)}
        />
      </div>
      <DetailBlock label="Working directory" value={event.cwd} />
      <DetailBlock label="Summary" value={event.summary} />
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
    <div className="min-w-0 rounded-lg border border-border bg-card px-3 py-2 text-foreground">
      <MarkdownMessage content={content || ' '} groupId={groupId} />
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
      <div className="flex items-center gap-1.5 text-xs text-amber-700">
        <PauseCircle className="h-3.5 w-3.5" />
        {event.message}
      </div>
    )
  }
  if (event.type === 'agent_error') {
    return (
      <div className="flex items-center gap-1.5 text-xs text-red-700">
        <XCircle className="h-3.5 w-3.5" />
        {event.message}
      </div>
    )
  }
  if (event.type === 'agent_handoff') {
    return (
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <GitBranch className="h-3.5 w-3.5" />
        {event.message}
      </div>
    )
  }
  // agent_silent
  return <div className="text-xs italic text-muted-foreground">{event.message}</div>
}

interface AgentBlock {
  kind: 'agent'
  agentId: string
  displayName: string
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
    const last = blocks[blocks.length - 1]
    if (last && last.kind === 'agent' && last.agentId === agentId) {
      last.events.push(event)
      last.lastAt = eventTime(event)
      if (displayName) last.displayName = displayName
    } else {
      blocks.push({
        kind: 'agent',
        agentId,
        displayName: displayName || 'Agent',
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

function AgentBlockView({
  block,
  groupId,
  onSubmitHumanInput,
  renderedInputRequests,
}: {
  block: AgentBlock
  groupId: string
  onSubmitHumanInput?: (content: string) => void
  renderedInputRequests: Set<string>
}) {
  const streaming = blockIsStreaming(block)
  return (
    <div className="flex w-full gap-3 px-4 py-2">
      <AgentAvatar name={block.displayName} className="mt-0.5" />
      <div className="flex min-w-0 max-w-[85%] flex-col gap-1.5">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{block.displayName}</span>
          {streaming ? (
            <span className="inline-flex items-center gap-1 text-amber-600">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />
              streaming
            </span>
          ) : (
            <span>{timeLabel(block.lastAt)}</span>
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          {block.events.map((event) => {
            if (event.type === 'agent_start') return null
            if (event.type === 'reasoning') {
              return <ReasoningBlock key={event.id} event={event} />
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
          })}
        </div>
      </div>
    </div>
  )
}

export function StreamTimeline({ run, onSubmitHumanInput }: StreamTimelineProps) {
  const blocks = buildBlocks(run.events)
  const renderedInputRequests = new Set<string>()
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
            groupId={run.group_id}
            onSubmitHumanInput={onSubmitHumanInput}
            renderedInputRequests={renderedInputRequests}
          />
        )
      })}
    </div>
  )
}
