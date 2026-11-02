import { memo, useMemo } from 'react'
import { Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

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
import { messageCopyText } from '@/lib/messageCopy'
import { replySegments } from '@/lib/replySegments'
import { normalizeLanguage } from '@/i18n'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message, TaskReport } from '@/types/api'
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

  if (message.sender_type === 'system' && message.message_type === 'task_report') {
    return <TaskReportCards message={message} groupId={groupId} />
  }
  if (message.sender_type === 'system' && message.message_type === 'watchdog_alert') {
    return <WatchdogAlertCard message={message} groupId={groupId} />
  }
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
  const content = message.content ?? ''
  // The shared copy-menu source, so right-clicking a message with uploads
  // yields the same text the copy button writes.
  const copyText = messageCopyText(content, message.attachments, t('messages.copyAttachments'))
  const contentSegments = isUser ? [content] : replySegments(content, message.response_segments)

  return (
    <div
      id={`message-${message.id}`}
      data-copy-text={copyText}
      data-sender={isUser ? 'user' : 'agent'}
      className={cn(
        'chat-message group/message flex min-w-0 w-full gap-2 px-3 py-2 transition-opacity',
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
        {/*
          `min-h-7` reserves the hover actions' height. They are the tallest
          thing in this row and they are absent while the row streams, is
          pending, or has no content yet — without the reservation the row
          grows once they appear and nudges the list under it.
        */}
        <div className="chat-message-meta flex min-h-7 min-w-0 items-center gap-2 text-xs text-muted-foreground">
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
              attachments={message.attachments}
            />
          )}
        </div>
        {!isUser && (!showStreamingDot || isResuming) && (
          <PersistedTurnDetails
            reasoning={message.reasoning}
            toolCalls={message.tool_calls}
          />
        )}
        {inputRequest ? (
          <div className="min-w-0 w-full max-w-full rounded-lg">
            <HumanInputRequestForm
              request={inputRequest}
              pin={isInterrupted}
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

const taskReportStatusClass: Record<TaskReport['status'], string> = {
  completed: 'text-emerald-600 dark:text-emerald-400',
  failed: 'text-destructive',
  waiting_for_user: 'text-amber-600 dark:text-amber-400',
}

/** Results of router-dispatched tasks, each linking to its task thread. */
function TaskReportCards({ message, groupId }: { message: Message; groupId: string }) {
  const { t } = useTranslation('chat')
  const reports = message.task_reports ?? []
  if (reports.length === 0) {
    // A live echo may arrive before the structured reports are refetched.
    return (
      <div className="my-2 text-center text-xs text-muted-foreground">{t('taskReport.label')}</div>
    )
  }
  return (
    <div
      id={`message-${message.id}`}
      className="mx-auto my-2 w-full max-w-2xl space-y-2 px-3"
      aria-label={t('taskReport.label')}
    >
      {reports.map((report) => (
        <div
          key={report.task_id}
          className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-sm"
        >
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate font-medium">
              {report.title || t('taskReport.untitled')}
            </span>
            {report.assistant ? (
              <span className="shrink-0 text-xs text-muted-foreground">{report.assistant}</span>
            ) : null}
            <span className={cn('ml-auto shrink-0 text-xs', taskReportStatusClass[report.status])}>
              {t(`taskReport.status.${report.status}`)}
            </span>
          </div>
          {report.excerpt ? (
            <p className="mt-1 line-clamp-3 whitespace-pre-wrap break-words text-xs text-muted-foreground">
              {report.excerpt}
            </p>
          ) : null}
          {report.status === 'waiting_for_user' ? (
            <p className="mt-1 text-xs text-amber-700 dark:text-amber-300">
              {t('taskReport.waitingHint')}
            </p>
          ) : null}
          <Link
            to={`/groups/${groupId}?thread=${encodeURIComponent(report.task_id)}`}
            className="mt-1 inline-block text-xs text-primary underline-offset-2 hover:underline"
          >
            {t('taskReport.open')}
          </Link>
        </div>
      ))}
    </div>
  )
}

/** A watchdog check's alert about a task, linking to the task thread. */
function WatchdogAlertCard({ message, groupId }: { message: Message; groupId: string }) {
  const { t } = useTranslation('chat')
  const alert = message.watchdog_alert
  if (!alert) {
    // A live echo may arrive before the structured alert is refetched.
    return (
      <div className="my-2 text-center text-xs text-muted-foreground">{message.content}</div>
    )
  }
  return (
    <div
      id={`message-${message.id}`}
      className="mx-auto my-2 w-full max-w-2xl px-3"
      aria-label={t('watchdogAlert.label')}
    >
      <div className="rounded-lg border border-amber-500/40 bg-amber-500/5 px-3 py-2 text-sm">
        <div className="flex min-w-0 items-center gap-2">
          <span className="shrink-0 text-xs font-medium text-amber-700 dark:text-amber-300">
            {t('watchdogAlert.label')}
          </span>
          <span className="truncate font-medium">
            {alert.title || t('taskReport.untitled')}
          </span>
          <span
            className={cn(
              'ml-auto shrink-0 text-xs',
              alert.action === 'stopped' ? 'text-destructive' : 'text-muted-foreground',
            )}
          >
            {t(`watchdogAlert.action.${alert.action}`)}
          </span>
        </div>
        <p className="mt-1 whitespace-pre-wrap break-words text-xs text-muted-foreground">
          {alert.message}
        </p>
        <Link
          to={`/groups/${groupId}?thread=${encodeURIComponent(alert.task_id)}`}
          className="mt-1 inline-block text-xs text-primary underline-offset-2 hover:underline"
        >
          {t('taskReport.open')}
        </Link>
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
