import { useEffect, useRef } from 'react'

import { MessageItem } from '@/components/chat/MessageItem'
import { cn } from '@/lib/utils'
import { useMessageStore, type ToolActivity } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface MessageListProps {
  groupId: string
}

const EMPTY_MESSAGES: readonly Message[] = []
const EMPTY_INFLIGHT: Record<string, never> = {}
const EMPTY_WARNINGS: readonly string[] = []
const EMPTY_TOOL_ACTIVITY: readonly ToolActivity[] = []

function toolStatusLabel(status: ToolActivity['status']): string {
  return status.replace(/_/g, ' ')
}

function toolStatusClasses(status: ToolActivity['status']): string {
  if (status === 'completed') return 'border-emerald-200 bg-emerald-50 text-emerald-700'
  if (status === 'started') return 'border-blue-200 bg-blue-50 text-blue-700'
  if (status === 'failed') return 'border-destructive/30 bg-destructive/10 text-destructive'
  if (status === 'input_required' || status === 'approval_required') {
    return 'border-amber-200 bg-amber-50 text-amber-700'
  }
  return 'border-border bg-background text-muted-foreground'
}

export function MessageList({ groupId }: MessageListProps) {
  const messages = useMessageStore((s) => s.byGroup[groupId] ?? EMPTY_MESSAGES)
  const inFlight = useMessageStore(
    (s) => s.inFlightByGroup[groupId] ?? EMPTY_INFLIGHT,
  )
  const activeAgent = useMessageStore((s) => s.activeAgentByGroup[groupId] ?? null)
  const warnings = useMessageStore(
    (s) => s.warningsByGroup[groupId] ?? EMPTY_WARNINGS,
  )
  const toolActivity = useMessageStore(
    (s) => s.toolActivityByGroup[groupId] ?? EMPTY_TOOL_ACTIVITY,
  )
  const endRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  }, [messages, inFlight, activeAgent, warnings, toolActivity])

  const inFlightBubbles: Message[] = Object.values(inFlight).map((bubble) => ({
    id: `inflight:${bubble.agent_id}`,
    group_id: groupId,
    thread_id: null,
    sender_type: 'agent',
    sender_id: bubble.agent_id,
    message_type: 'text',
    content: bubble.content,
    status: 'visible',
    refs: null,
    reply_to_message_id: null,
    created_at: new Date().toISOString(),
  }))

  const showThinking =
    activeAgent !== null &&
    Object.keys(inFlight).length === 0

  const progressLabel =
    activeAgent && activeAgent.total > 1
      ? ` (${activeAgent.index + 1}/${activeAgent.total})`
      : ''

  return (
    <div className="flex flex-1 flex-col overflow-y-auto py-4">
      {messages.length === 0 && inFlightBubbles.length === 0 && !showThinking && (
        <div className="flex flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground">
          No messages yet. Try sending <code>@AgentName hello</code> to start.
        </div>
      )}
      {messages.map((m) => (
        <MessageItem key={m.id} message={m} groupId={groupId} />
      ))}
      {inFlightBubbles.map((m) => (
        <MessageItem key={m.id} message={m} groupId={groupId} isStreaming />
      ))}
      {showThinking && (
        <div className="flex items-center gap-2 px-4 py-2 text-xs text-muted-foreground">
          <span className="inline-flex items-center gap-1.5">
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-blue-500" />
            <span className="font-medium text-foreground">{activeAgent.display_name}</span>
            is thinking{progressLabel}…
          </span>
        </div>
      )}
      {toolActivity.length > 0 && (
        <div className="mx-4 mt-2 space-y-2 rounded-md border border-border bg-muted/40 p-2 text-xs text-muted-foreground">
          {toolActivity.slice(-4).map((activity) => (
            <div key={activity.id} className="rounded-md border border-border bg-background/80 p-2">
              <div className="flex items-center justify-between gap-3">
                <span>
                  <span className="font-medium text-foreground">{activity.display_name || 'Agent'}</span>{' '}
                  {activity.status === 'started' ? 'is using' : 'used'}{' '}
                  <span className="font-medium text-foreground">{activity.tool_name || 'Unknown tool'}</span>
                </span>
                <span
                  className={cn(
                    'shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-medium capitalize',
                    toolStatusClasses(activity.status),
                  )}
                >
                  {toolStatusLabel(activity.status)}
                </span>
              </div>
              {activity.args_summary && (
                <div className="mt-1 break-words">
                  <span className="font-medium text-foreground">Args:</span> {activity.args_summary}
                </div>
              )}
              {activity.result_summary && (
                <div className="mt-1 break-words">
                  <span className="font-medium text-foreground">Result:</span>{' '}
                  {activity.result_summary}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
      {warnings.length > 0 && (
        <div className="mt-2 px-4 text-center text-xs text-amber-600">
          {warnings[warnings.length - 1]}
        </div>
      )}
      <div ref={endRef} />
    </div>
  )
}
