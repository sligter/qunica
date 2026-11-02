import {
  AgentActivityBubble,
  type ActivityReasoningSegment,
  type ActivityToolItem,
} from '@/components/chat/AgentActivityBubble'
import type { MessageToolCall } from '@/types/api'
import { useTranslation } from 'react-i18next'
import { toolDisplayName, type Translate } from '@/components/chat/toolDisplayName'

interface PersistedTurnDetailsProps {
  reasoning?: string[] | null
  toolCalls?: MessageToolCall[] | null
}

function persistedReasoning(reasoning: string[] | null | undefined): ActivityReasoningSegment[] {
  return (reasoning ?? []).map((content, index) => ({
    id: `persisted-reasoning-${index}`,
    content,
  }))
}

function persistedTools(toolCalls: MessageToolCall[] | null | undefined, t: Translate): ActivityToolItem[] {
  return (toolCalls ?? []).map((call, index) => ({
    id: call.tool_call_id ?? `persisted-tool-${index}`,
    name: toolDisplayName(call.tool_name, t, {
      argsSummary: call.args_summary,
      resultSummary: call.result_summary,
    }),
    status: call.status,
    argsSummary: call.args_summary,
    resultSummary: call.result_summary,
  }))
}

/**
 * Render persisted agent process metadata through one collapsed disclosure.
 *
 * The checklist is not part of it: the conversation's current plan renders
 * above the composer (`ConversationPlan`), not inside a reply.
 */
export function PersistedTurnDetails({ reasoning, toolCalls }: PersistedTurnDetailsProps) {
  const { t } = useTranslation('chat')
  return (
    <AgentActivityBubble
      reasoning={persistedReasoning(reasoning)}
      tools={persistedTools(toolCalls, t)}
    />
  )
}
