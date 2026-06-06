import {
  Bot,
  CheckCircle2,
  CircleAlert,
  Clock3,
  GitBranch,
  MessageSquareText,
  PauseCircle,
  Terminal,
  Wrench,
  XCircle,
} from 'lucide-react'
import type { ReactNode } from 'react'

import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { humanInputRequestFromText, normalizeHumanInputRequest } from '@/lib/humanInput'
import { cn } from '@/lib/utils'
import type {
  StreamExternalRunEvent,
  StreamNoticeEvent,
  StreamResponseDraftEvent,
  StreamRun,
  StreamRunStatus,
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

function runStatusLabel(status: StreamRunStatus): string {
  if (status === 'active') return 'Running'
  if (status === 'completed') return 'Completed'
  if (status === 'cancelled') return 'Cancelled'
  return 'Needs attention'
}

function runStatusClasses(status: StreamRunStatus): string {
  if (status === 'active') return 'border-blue-200 bg-blue-50 text-blue-700'
  if (status === 'completed') return 'border-emerald-200 bg-emerald-50 text-emerald-700'
  if (status === 'cancelled') return 'border-amber-200 bg-amber-50 text-amber-700'
  return 'border-red-200 bg-red-50 text-red-700'
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

function NoticeIcon({ event }: { event: StreamNoticeEvent }) {
  if (event.type === 'done') return <CheckCircle2 className="h-3.5 w-3.5" />
  if (event.type === 'agent_error') return <XCircle className="h-3.5 w-3.5" />
  if (event.type === 'waiting_for_user') return <PauseCircle className="h-3.5 w-3.5" />
  if (event.type === 'agent_handoff') return <GitBranch className="h-3.5 w-3.5" />
  return <CircleAlert className="h-3.5 w-3.5" />
}

function DetailBlock({ label, value }: { label: string; value: string | undefined }) {
  if (!value) return null
  return (
    <details className="mt-2 rounded-md border border-border bg-background">
      <summary className="cursor-pointer px-2.5 py-1.5 text-[11px] font-medium text-muted-foreground hover:text-foreground">
        {label}
      </summary>
      <pre className="max-h-44 overflow-auto whitespace-pre-wrap break-words border-t border-border px-2.5 py-2 text-xs leading-relaxed text-foreground">
        {value}
      </pre>
    </details>
  )
}

function EventShell({
  icon,
  children,
  muted = false,
}: {
  icon: ReactNode
  children: ReactNode
  muted?: boolean
}) {
  return (
    <div className="relative grid grid-cols-[1.75rem_minmax(0,1fr)] gap-2.5">
      <div className="relative flex justify-center">
        <span
          className={cn(
            'z-10 mt-1 flex h-6 w-6 items-center justify-center rounded-full border bg-background',
            muted ? 'border-border text-muted-foreground' : 'border-primary/30 text-primary',
          )}
        >
          {icon}
        </span>
      </div>
      <div className="min-w-0 pb-3">{children}</div>
    </div>
  )
}

function MetaLine({
  title,
  time,
  detail,
  badge,
  badgeClassName,
}: {
  title: string
  time: string
  detail?: string
  badge?: string
  badgeClassName?: string
}) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs">
      <span className="font-medium text-foreground">{title}</span>
      {detail ? <span className="min-w-0 break-words text-muted-foreground">{detail}</span> : null}
      <span className="text-muted-foreground">{time}</span>
      {badge ? (
        <span
          className={cn(
            'rounded-full border px-2 py-0.5 text-[10px] font-medium capitalize',
            badgeClassName,
          )}
        >
          {badge}
        </span>
      ) : null}
    </div>
  )
}

function ResponseEvent({
  event,
  onSubmitHumanInput,
}: {
  event: StreamResponseDraftEvent
  onSubmitHumanInput?: (content: string) => void
}) {
  const isStreaming = event.status === 'streaming'
  const inputRequest = humanInputRequestFromText(event.content)
  return (
    <EventShell icon={<MessageSquareText className="h-3.5 w-3.5" />}>
      <MetaLine
        title={event.display_name}
        detail={isStreaming ? 'is responding' : 'responded'}
        time={timeLabel(event.updated_at ?? event.created_at)}
        badge={isStreaming ? 'streaming' : 'final'}
        badgeClassName={
          isStreaming
            ? 'border-amber-200 bg-amber-50 text-amber-700'
            : 'border-emerald-200 bg-emerald-50 text-emerald-700'
        }
      />
      {inputRequest ? (
        <HumanInputRequestForm
          className="mt-2"
          request={inputRequest}
          targetDisplayName={event.display_name}
          onSubmitResponse={onSubmitHumanInput}
        />
      ) : (
        <div className="mt-2 min-w-0 rounded-md border border-border bg-card px-3 py-2">
          <MarkdownMessage content={event.content || ' '} />
        </div>
      )}
    </EventShell>
  )
}

function ToolEvent({
  event,
  onSubmitHumanInput,
}: {
  event: StreamToolEvent
  onSubmitHumanInput?: (content: string) => void
}) {
  const inputRequest = normalizeHumanInputRequest(
    event.input_request,
    event.result_summary,
    event.args_summary,
  )
  return (
    <EventShell icon={<Wrench className="h-3.5 w-3.5" />}>
      <MetaLine
        title={event.display_name}
        detail={`used ${event.tool_name}`}
        time={timeLabel(event.updated_at ?? event.created_at)}
        badge={toolStatusLabel(event.status)}
        badgeClassName={toolStatusClasses(event.status)}
      />
      {inputRequest ? (
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
    </EventShell>
  )
}

function ExternalRunEvent({ event }: { event: StreamExternalRunEvent }) {
  const adapter = event.adapter ? `External CLI: ${event.adapter}` : 'External CLI'
  const exitCode = event.exit_code === undefined ? '' : `exit ${event.exit_code}`
  return (
    <EventShell icon={<Terminal className="h-3.5 w-3.5" />}>
      <MetaLine
        title={event.display_name}
        detail={adapter}
        time={timeLabel(event.updated_at ?? event.created_at)}
        badge={event.status ?? exitCode}
        badgeClassName={externalStatusClasses(event.status)}
      />
      <DetailBlock label="Working directory" value={event.cwd} />
      <DetailBlock label="Summary" value={event.summary} />
    </EventShell>
  )
}

function NoticeEvent({
  event,
  onSubmitHumanInput,
}: {
  event: StreamNoticeEvent
  onSubmitHumanInput?: (content: string) => void
}) {
  const title = event.display_name ?? 'Stream'
  const inputRequest =
    event.type === 'waiting_for_user'
      ? normalizeHumanInputRequest(event.input_request, event.message)
      : null
  return (
    <EventShell icon={<NoticeIcon event={event} />} muted={event.type === 'done'}>
      <MetaLine
        title={title}
        detail={inputRequest ? 'waiting for your response' : event.message}
        time={timeLabel(event.created_at)}
      />
      {inputRequest ? (
        <HumanInputRequestForm
          className="mt-2"
          compact
          request={inputRequest}
          targetDisplayName={event.display_name}
          onSubmitResponse={onSubmitHumanInput}
        />
      ) : null}
    </EventShell>
  )
}

function renderEvent(
  event: StreamTimelineEvent,
  onSubmitHumanInput?: (content: string) => void,
) {
  if (event.type === 'agent_start') {
    const progress =
      event.total && event.total > 1
        ? `agent ${event.index === undefined ? '?' : event.index + 1}/${event.total}`
        : undefined
    const round = event.round ? `round ${event.round}` : undefined
    return (
      <EventShell key={event.id} icon={<Bot className="h-3.5 w-3.5" />}>
        <MetaLine
          title={event.display_name}
          detail={[progress, round, 'started'].filter(Boolean).join(' / ')}
          time={timeLabel(event.created_at)}
        />
      </EventShell>
    )
  }
  if (event.type === 'response_draft') {
    return <ResponseEvent key={event.id} event={event} onSubmitHumanInput={onSubmitHumanInput} />
  }
  if (event.type === 'tool') {
    return <ToolEvent key={event.id} event={event} onSubmitHumanInput={onSubmitHumanInput} />
  }
  if (event.type === 'external_run') {
    return <ExternalRunEvent key={event.id} event={event} />
  }
  if (event.type === 'agent_message') {
    const inputRequest = humanInputRequestFromText(event.content)
    return (
      <EventShell key={event.id} icon={<MessageSquareText className="h-3.5 w-3.5" />}>
        <MetaLine title={event.display_name} detail="responded" time={timeLabel(event.created_at)} />
        {inputRequest ? (
          <HumanInputRequestForm
            className="mt-2"
            request={inputRequest}
            targetDisplayName={event.display_name}
            onSubmitResponse={onSubmitHumanInput}
          />
        ) : (
          <div className="mt-2 min-w-0 rounded-md border border-border bg-card px-3 py-2">
            <MarkdownMessage content={event.content || ' '} />
          </div>
        )}
      </EventShell>
    )
  }
  return <NoticeEvent key={event.id} event={event} onSubmitHumanInput={onSubmitHumanInput} />
}

export function StreamTimeline({ run, onSubmitHumanInput }: StreamTimelineProps) {
  return (
    <section className="mx-3 mb-2 ml-12 max-w-3xl pr-3 sm:mx-4 sm:ml-14">
      <div className="relative rounded-md border border-border bg-muted/30 px-3 py-3">
        <div className="absolute bottom-3 left-[1.62rem] top-11 w-px bg-border" />
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Clock3 className="h-3.5 w-3.5" />
            <span>Execution timeline</span>
          </div>
          <span
            className={cn(
              'rounded-full border px-2 py-0.5 text-[10px] font-medium',
              runStatusClasses(run.status),
            )}
          >
            {runStatusLabel(run.status)}
          </span>
        </div>
        <div className="relative">
          {run.events.map((event) => renderEvent(event, onSubmitHumanInput))}
        </div>
      </div>
    </section>
  )
}
