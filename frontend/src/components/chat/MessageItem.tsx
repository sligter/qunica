import { useMemo } from 'react'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { InterruptedMessageActions } from '@/components/chat/InterruptedMessageActions'
import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { MessageActions } from '@/components/chat/MessageActions'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { humanInputRequestFromText } from '@/lib/humanInput'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface MessageItemProps {
  message: Message
  groupId: string
  isStreaming?: boolean
  onSubmitHumanInput?: (content: string) => void
}

export function MessageItem({
  message,
  groupId,
  isStreaming,
  onSubmitHumanInput,
}: MessageItemProps) {
  const groupAgents = useGroupAgents(groupId)
  const currentUser = useAuthStore((s) => s.user)
  const isResuming = useMessageStore((s) => s.resumingMessageIds.has(message.id))
  const groupAgent = useMemo(() => {
    if (message.sender_type !== 'agent') return undefined
    return groupAgents.data?.find((g) => g.agent_id === message.sender_id)
  }, [groupAgents.data, message.sender_id, message.sender_type])

  const senderName = useMemo(() => {
    if (message.sender_type === 'user') {
      if (currentUser && message.sender_id === currentUser.id) return 'You'
      return 'User'
    }
    if (message.sender_type === 'agent') {
      return groupAgent?.display_name ?? 'Agent'
    }
    return 'System'
  }, [currentUser, groupAgent?.display_name, message.sender_id, message.sender_type])

  if (message.sender_type === 'system') {
    return (
      <div className="my-2 text-center text-xs text-muted-foreground">
        {message.content}
      </div>
    )
  }

  const isUser = message.sender_type === 'user'
  const inputRequest = !isUser ? humanInputRequestFromText(message.content) : null
  const isInterrupted = message.status === 'interrupted'
  const showStreamingDot = isStreaming || isResuming
  const time = new Date(message.created_at).toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
  })

  return (
    <div
      id={`message-${message.id}`}
      className={cn(
        'group/message flex w-full gap-3 px-4 py-2',
        isUser ? 'flex-row-reverse' : 'flex-row',
      )}
    >
      <AgentAvatar
        name={senderName}
        kind={isUser ? 'user' : 'agent'}
        className="mt-0.5"
        contextUsage={message.context_usage ?? groupAgent?.context_usage ?? null}
      />
      <div
        className={cn(
          'flex max-w-[78%] flex-col gap-1',
          isUser ? 'items-end' : 'items-start',
        )}
      >
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{senderName}</span>
          {!showStreamingDot && !isInterrupted && <span>{time}</span>}
          {showStreamingDot && (
            <span className="inline-flex items-center gap-1 text-warning-foreground">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground" />
              streaming
            </span>
          )}
          {isInterrupted && !isResuming && (
            <span className="inline-flex items-center gap-1 text-warning-foreground">
              <span className="h-1.5 w-1.5 rounded-full bg-warning-foreground" />
              interrupted
            </span>
          )}
          {message.content && !showStreamingDot && (
            <MessageActions
              messageId={message.id}
              content={message.content}
              senderName={senderName}
              timeLabel={time}
              groupId={groupId}
            />
          )}
        </div>
        <div
          className={cn(
            'min-w-0 rounded-lg',
            inputRequest
              ? 'w-full'
              : isUser
                ? 'chat-user-bubble px-3 py-2'
                : 'border border-l-4 border-border border-l-primary/60 bg-card px-3 py-2 text-foreground shadow-sm',
            isInterrupted && !isResuming && 'border-warning',
          )}
        >
          {inputRequest ? (
            <HumanInputRequestForm
              request={inputRequest}
              targetDisplayName={senderName}
              onSubmitResponse={onSubmitHumanInput}
            />
          ) : (
            <MarkdownMessage content={message.content || ' '} isUser={isUser} groupId={groupId} />
          )}
        </div>
        {isInterrupted && !isResuming && message.thread_id && (
          <InterruptedMessageActions
            groupId={groupId}
            threadId={message.thread_id}
            messageId={message.id}
          />
        )}
      </div>
    </div>
  )
}
