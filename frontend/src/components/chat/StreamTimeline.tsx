import { useMemo, type ReactNode } from 'react'
import { GitBranch, PauseCircle, XCircle } from 'lucide-react'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import {
  AgentActivityBubble,
  type ActivityReasoningSegment,
  type ActivityToolItem,
} from '@/components/chat/AgentActivityBubble'
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
    <div className="inline-flex w-fit items-center gap-2 rounded-md border border-warning bg-warning px-2.5 py-1.5 text-xs text-warning-foreground">
      <span className="inline-flex items-center gap-0.5" aria-hidden="true">
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground" />
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground [animation-delay:140ms]" />
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground [animation-delay:280ms]" />
      </span>
      <span>{label}</span>
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
    <div className="w-fit max-w-full min-w-0 rounded-md border border-l-4 border-border border-l-primary/60 bg-card px-3 py-2 text-foreground shadow-sm">
      <MarkdownMessage content={content || ' '} groupId={groupId} />
    </div>
  )
}

function NoticeShell({
  tone,
  children,
}: {
  tone: 'warning' | 'destructive' | 'muted'
  children: ReactNode
}) {
  const classes =
    tone === 'destructive'
      ? 'border-destructive/30 border-l-destructive/70 bg-destructive/10 text-destructive'
      : tone === 'warning'
        ? 'border-warning border-l-warning-foreground/70 bg-warning text-warning-foreground'
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
      <NoticeShell tone="warning">
        <PauseCircle className="h-3.5 w-3.5" />
        {event.message}
      </NoticeShell>
    )
  }
  if (event.type === 'agent_error') {
    return (
      <NoticeShell tone="destructive">
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
      (event.type === 'tool' && event.status === 'started') ||
      (event.type === 'external_run' &&
        (event.status === 'running' ||
          (event.status === undefined && event.exit_code === undefined))),
  )
}

function blockIsWaiting(block: AgentBlock): boolean {
  return block.events.every((event) => event.type === 'agent_start')
}

function isActivityEvent(
  event: StreamTimelineEvent,
): event is StreamReasoningEvent | StreamToolEvent | StreamExternalRunEvent {
  return event.type === 'reasoning' || event.type === 'tool' || event.type === 'external_run'
}

function toolDefaultOpen(status: StreamToolEvent['status']): boolean {
  return (
    status === 'started' ||
    status === 'input_required' ||
    status === 'approval_required' ||
    status === 'failed'
  )
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
  const reasoning: ActivityReasoningSegment[] = block.events.flatMap((event) =>
    event.type === 'reasoning'
      ? [
          {
            id: event.id,
            content: event.content,
            streaming: event.status === 'streaming',
          },
        ]
      : [],
  )
  const visibleInputRequestKeys = new Set(
    block.events.flatMap((event) => {
      if (event.type !== 'waiting_for_user') return []
      const request = normalizeHumanInputRequest(event.input_request, event.message)
      return request ? [inputRequestKey(request)] : []
    }),
  )
  const tools: ActivityToolItem[] = block.events.flatMap(
    (event): ActivityToolItem[] => {
      if (event.type === 'tool') {
        const inputRequest = normalizeHumanInputRequest(
          event.input_request,
          event.result_summary,
          event.args_summary,
        )
        const keepInputRequestVisible =
          inputRequest && visibleInputRequestKeys.has(inputRequestKey(inputRequest))
        const details =
          inputRequest &&
          !keepInputRequestVisible &&
          shouldRenderInputRequest(inputRequest, renderedInputRequests) ? (
            <HumanInputRequestForm
              compact
              request={inputRequest}
              targetDisplayName={event.display_name}
              onSubmitResponse={onSubmitHumanInput}
            />
          ) : undefined
        return [
          {
            id: event.id,
            name: event.tool_name,
            status: event.status,
            argsSummary: event.args_summary,
            resultSummary: event.result_summary,
            details,
            defaultOpen: toolDefaultOpen(event.status),
          },
        ]
      }
      if (event.type === 'external_run') {
        const exitCode = event.exit_code === undefined ? null : `exit ${event.exit_code}`
        return [
          {
            id: event.id,
            name: event.adapter ? `External CLI: ${event.adapter}` : 'External CLI',
            status: event.status ?? exitCode ?? 'running',
            argsSummary: event.cwd,
            resultSummary: event.summary,
            argsLabel: 'Working directory',
            resultLabel: 'Summary',
            defaultOpen:
              event.status === 'running' ||
              (event.status === undefined && event.exit_code === undefined),
            kind: 'external' as const,
          },
        ]
      }
      return []
    },
  )
  const activityActive = block.events.some(
    (event) =>
      (event.type === 'reasoning' && event.status === 'streaming') ||
      (event.type === 'tool' && event.status === 'started') ||
      (event.type === 'external_run' &&
        (event.status === 'running' ||
          (event.status === undefined && event.exit_code === undefined))),
  )
  const visibleEvents = block.events.filter((event) => !isActivityEvent(event))
  const renderEvent = (event: StreamTimelineEvent): ReactNode => {
    if (event.type === 'agent_start') return null
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
    if (isActivityEvent(event)) return null
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
            <span className="inline-flex items-center gap-1 rounded-[3px] border border-warning bg-warning px-1.5 py-0.5 text-[10px] text-warning-foreground">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground" />
              streaming
            </span>
          ) : (
            <span>{timeLabel(block.lastAt)}</span>
          )}
        </div>
        <div className="flex min-w-0 max-w-full flex-col items-start gap-1.5">
          {waiting ? <WaitingHint label="Preparing response..." /> : null}
          <AgentActivityBubble reasoning={reasoning} tools={tools} active={activityActive} />
          {visibleEvents.map(renderEvent)}
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
            <span className="inline-flex items-center gap-1 rounded-[3px] border border-warning bg-warning px-1.5 py-0.5 text-[10px] text-warning-foreground">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground" />
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
