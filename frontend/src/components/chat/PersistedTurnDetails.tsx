import {
  AgentActivityBubble,
  type ActivityReasoningSegment,
  type ActivityToolItem,
} from '@/components/chat/AgentActivityBubble'
import type { MessageToolCall } from '@/types/api'

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

function persistedTools(toolCalls: MessageToolCall[] | null | undefined): ActivityToolItem[] {
  return (toolCalls ?? []).map((call, index) => ({
    id: call.tool_call_id ?? `persisted-tool-${index}`,
    name: call.tool_name ?? 'Unknown tool',
    status: call.status,
    argsSummary: call.args_summary,
    resultSummary: call.result_summary,
  }))
}

/** Render persisted agent process metadata through one collapsed disclosure. */
export function PersistedTurnDetails({ reasoning, toolCalls }: PersistedTurnDetailsProps) {
  return (
    <AgentActivityBubble
      reasoning={persistedReasoning(reasoning)}
      tools={persistedTools(toolCalls)}
    />
  )
}
