import {
  AgentActivityBubble,
  type ActivityReasoningSegment,
  type ActivityToolItem,
} from '@/components/chat/AgentActivityBubble'
import { TodoChecklist } from '@/components/chat/TodoChecklist'
import type { MessageToolCall, TodoItem } from '@/types/api'
import { useTranslation } from 'react-i18next'

interface PersistedTurnDetailsProps {
  reasoning?: string[] | null
  toolCalls?: MessageToolCall[] | null
  todos?: TodoItem[] | null
}

function persistedReasoning(reasoning: string[] | null | undefined): ActivityReasoningSegment[] {
  return (reasoning ?? []).map((content, index) => ({
    id: `persisted-reasoning-${index}`,
    content,
  }))
}

function persistedTools(toolCalls: MessageToolCall[] | null | undefined, unknownTool: string): ActivityToolItem[] {
  return (toolCalls ?? []).map((call, index) => ({
    id: call.tool_call_id ?? `persisted-tool-${index}`,
    name: call.tool_name ?? unknownTool,
    status: call.status,
    argsSummary: call.args_summary,
    resultSummary: call.result_summary,
  }))
}

/**
 * Render persisted agent process metadata through one collapsed disclosure.
 *
 * The checklist sits outside that disclosure: where the turn got to is the part
 * of a finished turn a reader still wants at a glance, unlike the tool calls
 * they only open when something looks wrong.
 */
export function PersistedTurnDetails({ reasoning, toolCalls, todos }: PersistedTurnDetailsProps) {
  const { t } = useTranslation('chat')
  return (
    <>
      <AgentActivityBubble
        reasoning={persistedReasoning(reasoning)}
        tools={persistedTools(toolCalls, t('messages.unknownTool'))}
      />
      <TodoChecklist todos={todos ?? []} />
    </>
  )
}
