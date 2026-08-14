import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { InterruptedMessageActions } from '@/components/chat/InterruptedMessageActions'
import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { MessageActions } from '@/components/chat/MessageActions'
import { MessageAttachments } from '@/components/chat/MessageAttachments'
import { PersistedTurnDetails } from '@/components/chat/PersistedTurnDetails'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { humanInputRequestFromText } from '@/lib/humanInput'
import { formatTime } from '@/lib/format'
import { normalizeLanguage } from '@/i18n'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { GroupAgentRead } from '@/types/api'
import type { ConversationScope } from '@/hooks/useGroupMessages'

interface MessageItemProps {
  message: Message
  groupId: string
  isStreaming?: boolean
  onSubmitHumanInput?: (content: string) => void
  scope?: ConversationScope
  agents?: GroupAgentRead[]
  agentIsSystem?: boolean
}

export function MessageItem({
  message,
  groupId,
  isStreaming,
  onSubmitHumanInput,
  scope = 'groups',
  agents,
  agentIsSystem,
}: MessageItemProps) {
  const { t, i18n } = useTranslation('chat')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  // The conversation view already resolved the roster and passes it down; only
  // fall back to a query when a caller renders a message without one.
  const groupAgents = useGroupAgents(
    agents === undefined && scope === 'groups' ? groupId : undefined,
  )
  const currentUser = useAuthStore((s) => s.user)
  const isResuming = useMessageStore((s) => s.resumingMessageIds.has(message.id))
  const groupAgent = useMemo(() => {
    if (message.sender_type !== 'agent') return undefined
    return (agents ?? groupAgents.data)?.find((g) => g.agent_id === message.sender_id)
  }, [agents, groupAgents.data, message.sender_id, message.sender_type])

  const openFile = useFileNavStore((s) => s.openFile)
  // Only a non-default mode is worth badging: it means this agent's files are
  // not where the panel shows by default.
  const workspaceModeKey =
    groupAgent?.workspace_mode === 'self'
      ? 'messages.workspaceIsolated'
      : groupAgent?.workspace_mode === 'group_and_self'
        ? 'messages.workspaceOwnFolder'
        : null

  const senderName = useMemo(() => {
    if (message.sender_type === 'user') {
      if (currentUser && message.sender_id === currentUser.id) return t('messages.you')
      return t('messages.user')
    }
    if (message.sender_type === 'agent') {
      return groupAgent?.display_name ?? t('messages.agent')
    }
    return t('messages.system')
  }, [currentUser, groupAgent?.display_name, message.sender_id, message.sender_type, t])

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
  const time = formatTime(message.created_at, language)
  const persistedSegments = message.response_segments?.filter((segment) => segment.length > 0) ?? []
  const content = message.content ?? ''
  const contentSegments = !isUser && persistedSegments.length > 0 && persistedSegments.join('') === content
    ? persistedSegments
    : [content]

  return (
    <div
      id={`message-${message.id}`}
      className={cn(
        'group/message flex min-w-0 w-full gap-2 px-3 py-2',
        isUser ? 'flex-row-reverse' : 'flex-row',
      )}
    >
      <AgentAvatar
        name={senderName}
        kind={isUser ? 'user' : agentIsSystem ? 'system' : 'agent'}
        className="mt-0.5"
        contextUsage={message.context_usage ?? groupAgent?.context_usage ?? null}
      />
      <div
        className={cn(
          'flex min-w-0 flex-1 flex-col gap-1',
          isUser ? 'ml-auto max-w-[72%] items-end' : 'max-w-full items-start',
        )}
      >
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{senderName}</span>
          {workspaceModeKey ? (
            <button
              type="button"
              className="rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-muted"
              onClick={() => openFile(groupId, '', message.sender_id)}
              title={t(workspaceModeKey)}
            >
              {t(workspaceModeKey)}
            </button>
          ) : null}
          {!showStreamingDot && !isInterrupted && <span>{time}</span>}
          {showStreamingDot && (
            <span className="inline-flex items-center gap-1 text-warning-foreground">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-warning-foreground" />
              {t('messages.streaming')}
            </span>
          )}
          {isInterrupted && !isResuming && (
            <span className="inline-flex items-center gap-1 text-warning-foreground">
              <span className="h-1.5 w-1.5 rounded-full bg-warning-foreground" />
              {t('messages.interrupted')}
            </span>
          )}
          {message.content && !showStreamingDot && (
            <MessageActions
              messageId={message.id}
              content={message.content}
              senderName={senderName}
              timeLabel={time}
              groupId={groupId}
              scope={scope}
            />
          )}
        </div>
        {!isUser && !showStreamingDot && (
          <PersistedTurnDetails
            reasoning={message.reasoning}
            toolCalls={message.tool_calls}
          />
        )}
        {inputRequest ? (
          <div className="min-w-0 w-full max-w-full rounded-lg">
            <HumanInputRequestForm
              request={inputRequest}
              targetDisplayName={senderName}
              onSubmitResponse={onSubmitHumanInput}
            />
          </div>
        ) : contentSegments.map((segment, index) => (
          <div
            key={index}
            className={cn(
              'min-w-0 max-w-full rounded-lg',
              isUser
                ? 'chat-user-bubble px-3 py-2'
                : 'border border-l-4 border-border border-l-primary/60 bg-card px-3 py-2 text-foreground shadow-sm',
              isInterrupted && !isResuming && 'border-warning',
            )}
          >
            <MarkdownMessage content={segment || ' '} isUser={isUser} groupId={groupId} />
            {index === contentSegments.length - 1 && message.attachments.length > 0 ? (
              <MessageAttachments groupId={groupId} attachments={message.attachments} scope={scope} />
            ) : null}
          </div>
        ))}
        {(isInterrupted || isResuming) && message.thread_id && (
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
