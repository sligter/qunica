import { useTranslation } from 'react-i18next'

import { ToolApprovalCard } from '@/components/chat/ToolApprovalCard'
import { conversationStateKey } from '@/hooks/useGroupMessages'
import { useResumeStream } from '@/hooks/useResumeStream'
import { useMessageStore, type StreamApprovalRequest } from '@/stores/messageStore'

interface ToolApprovalPromptProps {
  groupId: string
  threadId?: string
  request: StreamApprovalRequest
  resolved?: 'approved' | 'declined'
}

/**
 * The approval card, wired to the resume stream.
 *
 * The wiring lives here rather than in the conversation view so the resume hook
 * — and the query client it needs — is only mounted when there is actually an
 * approval to answer. Answering continues the paused thread; the runtime replays
 * the recorded call, so the command that runs is the one this card showed.
 *
 * A turn paused for approval leaves exactly one interrupted message, and the
 * resume endpoint selects the latest one by the same rule this does.
 */
export function ToolApprovalPrompt({
  groupId,
  threadId,
  request,
  resolved,
}: ToolApprovalPromptProps) {
  const { t } = useTranslation('chat')
  const stateId = conversationStateKey(groupId, threadId)
  const messageId = useMessageStore((state) => {
    const messages = stateId ? state.byGroup?.[stateId] : undefined
    if (!messages) return undefined
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index].status === 'interrupted') return messages[index].id
    }
    return undefined
  })
  const { resume, isStreaming, error } = useResumeStream(groupId, threadId, messageId)

  return (
    <div className="min-w-0">
      <ToolApprovalCard
        request={request}
        resolved={resolved}
        onAnswer={messageId && !isStreaming ? resume : undefined}
      />
      {error ? (
        <div className="mt-1 text-xs text-destructive">
          {t('messages.resumeFailed', { message: error })}
        </div>
      ) : null}
    </div>
  )
}
