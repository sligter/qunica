import { useTranslation } from 'react-i18next'

import { ToolApprovalCard } from '@/components/chat/ToolApprovalCard'
import { useResumeStream } from '@/hooks/useResumeStream'
import { useMessageStore, type StreamApprovalRequest } from '@/stores/messageStore'
import type { ConversationScope } from '@/hooks/useGroupMessages'

interface ToolApprovalPromptProps {
  groupId: string
  /** The store bucket this conversation is read through. */
  stateId?: string
  request: StreamApprovalRequest
  resolved?: 'approved' | 'declined'
  scope?: ConversationScope
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
  stateId,
  request,
  resolved,
  scope = 'groups',
}: ToolApprovalPromptProps) {
  const { t } = useTranslation('chat')
  const messageId = useMessageStore((state) => {
    const messages = stateId ? state.byGroup?.[stateId] : undefined
    if (!messages) return undefined
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index].status === 'interrupted') return messages[index].id
    }
    return undefined
  })
  const { resume, isStreaming, error } = useResumeStream(groupId, stateId, messageId, scope)

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
