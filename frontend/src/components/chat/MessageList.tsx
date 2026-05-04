import { useEffect, useRef } from 'react'

import { MessageItem } from '@/components/chat/MessageItem'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface MessageListProps {
  groupId: string
}

const EMPTY_MESSAGES: readonly Message[] = []
const EMPTY_INFLIGHT: Record<string, never> = {}
const EMPTY_WARNINGS: readonly string[] = []

export function MessageList({ groupId }: MessageListProps) {
  const messages = useMessageStore((s) => s.byGroup[groupId] ?? EMPTY_MESSAGES)
  const inFlight = useMessageStore(
    (s) => s.inFlightByGroup[groupId] ?? EMPTY_INFLIGHT,
  )
  const activeAgent = useMessageStore((s) => s.activeAgentByGroup[groupId] ?? null)
  const warnings = useMessageStore(
    (s) => s.warningsByGroup[groupId] ?? EMPTY_WARNINGS,
  )
  const endRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  }, [messages, inFlight, activeAgent, warnings])

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
      {warnings.length > 0 && (
        <div className="mt-2 px-4 text-center text-xs text-amber-600">
          {warnings[warnings.length - 1]}
        </div>
      )}
      <div ref={endRef} />
    </div>
  )
}
