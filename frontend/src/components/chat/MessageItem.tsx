import { memo, useMemo } from 'react'
import { Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { InterruptedMessageActions } from '@/components/chat/InterruptedMessageActions'
import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { MessageActions } from '@/components/chat/MessageActions'
import { MessageAttachments } from '@/components/chat/MessageAttachments'
import { PersistedTurnDetails } from '@/components/chat/PersistedTurnDetails'
import { StreamStatusPill } from '@/components/chat/StreamStatus'
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
  /** The store bucket this conversation is read through. */
  stateId?: string
  /**
   * The thread the surrounding view's message query is keyed by — `undefined`
   * for conversations read without one, such as direct chats. Distinct from
   * `message.thread_id`, which the backend always fills in: keying a cache
   * write by the latter would miss the list actually on screen.
   */
  threadId?: string
  isStreaming?: boolean
  onSubmitHumanInput?: (content: string) => void
  scope?: ConversationScope
  agents?: GroupAgentRead[]
  agentIsSystem?: boolean
}

export function MessageItemView({
  message,
  groupId,
  stateId = groupId,
  threadId,
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
    message.thread_id ?? undefined,
  )
  const currentUser = useAuthStore((s) => s.user)
  const isResuming = useMessageStore((s) => s.resumingMessageIds.has(message.id))
  // A locally echoed message the server has not acknowledged yet. It renders
  // like any other message, dimmed, so sending never blocks on the round trip.
  const isPending = useMessageStore((s) => s.pendingMessageIds.has(message.id))
  const roster = agents ?? groupAgents.data
  const mentionNames = useMemo(
    () => scope === 'groups' ? (roster ?? []).map((agent) => agent.display_name) : [],
    [roster, scope],
  )
  const groupAgent = useMemo(() => {
    if (message.sender_type !== 'agent') return undefined
    return roster?.find((g) => g.agent_id === message.sender_id)
  }, [message.sender_id, message.sender_type, roster])

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
      data-copy-text={content}
      className={cn(
        'group/message flex min-w-0 w-full gap-2 px-3 py-2 transition-opacity',
        isUser ? 'flex-row-reverse' : 'flex-row',
        isPending && 'opacity-70',
      )}
    >
      <AgentAvatar
        name={isUser && message.sender_id === currentUser?.id ? currentUser.name : senderName}
        kind={isUser ? 'user' : agentIsSystem ? 'system' : 'agent'}
        agentId={!isUser && !agentIsSystem ? message.sender_id ?? undefined : undefined}
        conversationId={groupId}
        avatarUrl={
          isUser && message.sender_id === currentUser?.id
            ? currentUser.avatar_url
            : groupAgent?.avatar_url
        }
        className="mt-0.5"
        contextUsage={message.context_usage ?? groupAgent?.context_usage ?? null}
      />
      <div
        className={cn(
          'flex min-w-0 flex-1 flex-col gap-1',
          isUser ? 'ml-auto max-w-[72%] items-end' : 'max-w-full items-start',
        )}
      >
        <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
          <span className="shrink-0 font-medium text-foreground">{senderName}</span>
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
          {!showStreamingDot && !isInterrupted && !isPending && <span>{time}</span>}
          {isPending && (
            <span className="inline-flex items-center gap-1.5 text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
              {t('messages.sending')}
            </span>
          )}
          {showStreamingDot && (
            <StreamStatusPill status={{ phase: 'writing' }} className="min-w-0" />
          )}
          {isInterrupted && !isResuming && (
            <span className="inline-flex items-center gap-1 text-warning-foreground">
              <span className="h-1.5 w-1.5 rounded-full bg-warning-foreground" />
              {t('messages.interrupted')}
            </span>
          )}
          {message.content && !showStreamingDot && !isPending && (
            <MessageActions
              messageId={message.id}
              content={message.content}
              senderName={senderName}
              timeLabel={time}
              groupId={groupId}
              threadId={threadId}
              scope={scope}
            />
          )}
        </div>
        {!isUser && (!showStreamingDot || isResuming) && (
          <PersistedTurnDetails
            reasoning={message.reasoning}
            toolCalls={message.tool_calls}
            todos={message.todos}
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
            <MarkdownMessage
              content={segment || ' '}
              isUser={isUser}
              groupId={groupId}
              mentionNames={mentionNames}
            />
            {index === contentSegments.length - 1 && message.attachments.length > 0 ? (
              <MessageAttachments groupId={groupId} attachments={message.attachments} scope={scope} />
            ) : null}
          </div>
        ))}
        {(isInterrupted || isResuming) && message.thread_id && (
          <InterruptedMessageActions
            groupId={groupId}
            stateId={stateId}
            messageId={message.id}
            toolCalls={message.tool_calls}
            scope={scope}
          />
        )}
      </div>
    </div>
  )
}

/**
 * Memoized: every streamed token updates the message store, and an unmemoized
 * row would re-render — and re-parse the markdown of — the whole backlog on
 * each one. Message objects keep their identity in the store, so a row only
 * re-renders when its own content or state actually changed.
 */
export const MessageItem = memo(MessageItemView)
