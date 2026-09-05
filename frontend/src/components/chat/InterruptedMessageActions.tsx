import { Play } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ToolApprovalCard } from '@/components/chat/ToolApprovalCard'
import { Button } from '@/components/ui/button'
import { useResumeStream } from '@/hooks/useResumeStream'
import { MAX_RETRY_ATTEMPTS } from '@/lib/api-v2/retry'
import { useMessageStore, type StreamApprovalRequest } from '@/stores/messageStore'
import type { ConversationScope } from '@/hooks/useGroupMessages'
import type { MessageToolCall } from '@/types/api'

interface InterruptedMessageActionsProps {
  groupId: string
  /** The store bucket this conversation is read through. */
  stateId: string
  messageId: string
  /** The checkpointed calls of the interrupted turn, newest last. */
  toolCalls?: MessageToolCall[] | null
  scope?: ConversationScope
}

/**
 * The pause this message is waiting on, rebuilt from its checkpoint.
 *
 * A turn stops at one call at a time, so the pending one is the last call that
 * still carries a question and no result. Anything earlier was already answered
 * and ran. The rule matches the one the runtime replays by, so a card that
 * appears here is a card the resume endpoint will actually act on.
 */
function pendingApproval(
  toolCalls: MessageToolCall[] | null | undefined,
): StreamApprovalRequest | null {
  if (!toolCalls) return null
  for (let index = toolCalls.length - 1; index >= 0; index -= 1) {
    const call = toolCalls[index]
    if (call.status !== 'approval_required' || call.result_summary) continue
    if (!call.tool_call_id || !call.approval_request) continue
    return { ...call.approval_request, tool_call_id: call.tool_call_id }
  }
  return null
}

export function InterruptedMessageActions({
  groupId,
  stateId,
  messageId,
  toolCalls,
  scope = 'groups',
}: InterruptedMessageActionsProps) {
  const { t } = useTranslation('chat')
  const { resume, isStreaming, error, retry, retryExhausted } = useResumeStream(
    groupId,
    stateId,
    messageId,
    scope,
  )
  const approval = pendingApproval(toolCalls)
  const liveCardExists = useMessageStore((s) =>
    approval ? s.hasStreamApprovalNotice(approval.tool_call_id) : false,
  )
  if (isStreaming && !retry && !retryExhausted) return null

  // A turn stopped at a gate is not continuable — pressing continue only invites
  // the model to propose the command again, and the user would answer the same
  // question one round later. The card is the way forward, so it replaces the
  // button rather than sitting beside it.
  const showApproval = Boolean(approval) && !liveCardExists
  const showContinue = !isStreaming && !showApproval
  const showStatus = Boolean(retry) || Boolean(error)

  return (
    <div className="flex min-w-0 w-full max-w-full flex-col gap-2 text-xs">
      {showApproval && approval ? (
        <ToolApprovalCard request={approval} onAnswer={isStreaming ? undefined : resume} error={error} />
      ) : null}
      {showContinue || showStatus ? (
        <div className="flex items-center gap-2">
          {/* `resume` is wrapped rather than passed straight to onClick: it takes an
              optional approval answer, so handing it over directly would send
              React's click event as the decision this turn is waiting on. */}
          {showContinue ? (
            <Button size="sm" variant="outline" onClick={() => resume()} className="h-7 gap-1.5">
              <Play className="h-3 w-3" />
              {t('messages.continue')}
            </Button>
          ) : null}
          {retry ? (
            <span role="status" aria-live="polite" className="text-warning-foreground">
              {t('stream.reconnecting', { attempt: retry.attempt, max: MAX_RETRY_ATTEMPTS })}
            </span>
          ) : null}
          {retryExhausted ? (
            <Button size="sm" variant="outline" onClick={() => window.dispatchEvent(new Event('qunica:reconnect'))}>
              {t('common:mobile.reconnect')}
            </Button>
          ) : null}
          {error ? (
            <span className="text-destructive">
              {retryExhausted
                ? t('stream.retryExhausted', { max: MAX_RETRY_ATTEMPTS })
                : t('messages.resumeFailed', { message: error })}
            </span>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
