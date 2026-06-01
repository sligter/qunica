/**
 * Message store — sink for the SSE message stream.
 *
 * `byGroup` is the canonical chronological history per group; populated from
 * `GET /messages` on mount and appended to as `user_message` / `agent_message`
 * events arrive. `inFlightByGroup` holds the partial bubbles while tokens
 * are still streaming, keyed per (groupId, agentId) within a single send
 * batch. When the matching `agent_message` event arrives we drop the
 * in-flight entry and append the persisted Message to `byGroup`.
 */

import { create } from 'zustand'

import type { Message } from '@/types/api'

export interface StreamingBubble {
  id: string
  agent_id: string
  stream_id: string | null
  content: string
}

export interface ActiveAgent {
  agent_id: string
  display_name: string
  index: number
  total: number
  stream_id?: string | null
}

export type ToolActivityStatus =
  | 'started'
  | 'completed'
  | 'failed'
  | 'unavailable'
  | 'setup_required'
  | 'workspace_required'
  | 'input_required'
  | 'approval_required'

export interface ToolActivity {
  id: string
  agent_id: string
  display_name: string
  tool_name: string
  status: ToolActivityStatus
  args_summary?: string
  result_summary?: string
}

interface MessageState {
  byGroup: Record<string, Message[]>
  inFlightByGroup: Record<string, Record<string, StreamingBubble>>
  activeAgentsByGroup: Record<string, Record<string, ActiveAgent>>
  warningsByGroup: Record<string, string[]>
  toolActivityByGroup: Record<string, ToolActivity[]>
  resumingMessageIds: Set<string>

  setHistory: (groupId: string, messages: Message[]) => void
  prependHistory: (groupId: string, messages: Message[]) => void
  clearGroupMessages: (groupId: string) => void
  appendMessage: (groupId: string, message: Message) => void
  patchInFlight: (groupId: string, agentId: string, delta: string, streamId?: string | null) => void
  finalizeInFlight: (groupId: string, message: Message) => void
  clearInFlight: (groupId: string) => void
  clearStreamInFlight: (groupId: string, streamId: string) => void
  clearAgentInFlight: (groupId: string, agentId: string, streamId?: string | null) => void
  setActiveAgent: (groupId: string, agent: ActiveAgent) => void
  clearActiveAgent: (groupId: string, agentId?: string, streamId?: string | null) => void
  pushWarning: (groupId: string, warning: string) => void
  clearWarnings: (groupId: string) => void
  pushToolActivity: (groupId: string, activity: ToolActivity) => void
  clearToolActivity: (groupId: string) => void
  appendToMessage: (groupId: string, messageId: string, delta: string) => void
  replaceMessage: (groupId: string, message: Message) => void
  startResume: (messageId: string) => void
  endResume: (messageId: string) => void
}

function inFlightKey(agentId: string, streamId: string | null | undefined): string {
  return `${streamId ?? 'default'}:${agentId}`
}

export const useMessageStore = create<MessageState>((set) => ({
  byGroup: {},
  inFlightByGroup: {},
  activeAgentsByGroup: {},
  warningsByGroup: {},
  toolActivityByGroup: {},
  resumingMessageIds: new Set(),

  setHistory: (groupId, messages) =>
    set((s) => ({
      byGroup: { ...s.byGroup, [groupId]: messages },
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
      warningsByGroup: { ...s.warningsByGroup, [groupId]: [] },
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  prependHistory: (groupId, messages) =>
    set((s) => {
      const existing = s.byGroup[groupId] ?? []
      const existingIds = new Set(existing.map((message) => message.id))
      const older = messages.filter((message) => !existingIds.has(message.id))
      return {
        byGroup: { ...s.byGroup, [groupId]: [...older, ...existing] },
      }
    }),

  clearGroupMessages: (groupId) =>
    set((s) => ({
      byGroup: { ...s.byGroup, [groupId]: [] },
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
      warningsByGroup: { ...s.warningsByGroup, [groupId]: [] },
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  appendMessage: (groupId, message) =>
    set((s) => ({
      byGroup: {
        ...s.byGroup,
        [groupId]: [...(s.byGroup[groupId] ?? []), message],
      },
    })),

  patchInFlight: (groupId, agentId, delta, streamId = null) =>
    set((s) => {
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const bubbleId = inFlightKey(agentId, streamId)
      const existing = groupInFlight[bubbleId]
      const next: StreamingBubble = existing
        ? { ...existing, content: existing.content + delta }
        : { id: bubbleId, agent_id: agentId, stream_id: streamId, content: delta }
      return {
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: { ...groupInFlight, [bubbleId]: next },
        },
      }
    }),

  finalizeInFlight: (groupId, message) =>
    set((s) => {
      const agentId = message.sender_id ?? ''
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remaining = { ...groupInFlight }
      delete remaining[inFlightKey(agentId, message.reply_to_message_id)]
      delete remaining[inFlightKey(agentId, null)]
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const remainingActive = { ...groupActive }
      delete remainingActive[inFlightKey(agentId, message.reply_to_message_id)]
      delete remainingActive[inFlightKey(agentId, null)]
      return {
        byGroup: {
          ...s.byGroup,
          [groupId]: [...(s.byGroup[groupId] ?? []), message],
        },
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: remaining,
        },
        activeAgentsByGroup: {
          ...s.activeAgentsByGroup,
          [groupId]: remainingActive,
        },
      }
    }),

  clearInFlight: (groupId) =>
    set((s) => ({
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  clearStreamInFlight: (groupId, streamId) =>
    set((s) => {
      const streamPrefix = `${streamId}:`
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remainingInFlight = Object.fromEntries(
        Object.entries(groupInFlight).filter(([key]) => !key.startsWith(streamPrefix)),
      )
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const remainingActive = Object.fromEntries(
        Object.entries(groupActive).filter(([key]) => !key.startsWith(streamPrefix)),
      )
      return {
        inFlightByGroup: { ...s.inFlightByGroup, [groupId]: remainingInFlight },
        activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: remainingActive },
      }
    }),

  clearAgentInFlight: (groupId, agentId, streamId = null) =>
    set((s) => {
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remaining = { ...groupInFlight }
      delete remaining[inFlightKey(agentId, streamId)]
      if (streamId !== null) {
        delete remaining[inFlightKey(agentId, null)]
      }
      return {
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: remaining,
        },
      }
    }),

  setActiveAgent: (groupId, agent) =>
    set((s) => {
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const key = inFlightKey(agent.agent_id, agent.stream_id ?? null)
      return {
        activeAgentsByGroup: {
          ...s.activeAgentsByGroup,
          [groupId]: { ...groupActive, [key]: agent },
        },
      }
    }),

  clearActiveAgent: (groupId, agentId, streamId = null) =>
    set((s) => {
      if (!agentId) {
        if (streamId !== null) {
          const streamPrefix = `${streamId}:`
          const groupActive = s.activeAgentsByGroup[groupId] ?? {}
          const remaining = Object.fromEntries(
            Object.entries(groupActive).filter(([key]) => !key.startsWith(streamPrefix)),
          )
          return {
            activeAgentsByGroup: {
              ...s.activeAgentsByGroup,
              [groupId]: remaining,
            },
          }
        }
        return {
          activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
        }
      }
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const remaining = { ...groupActive }
      delete remaining[inFlightKey(agentId, streamId)]
      if (streamId !== null) {
        delete remaining[inFlightKey(agentId, null)]
      }
      return {
        activeAgentsByGroup: {
          ...s.activeAgentsByGroup,
          [groupId]: remaining,
        },
      }
    }),

  pushWarning: (groupId, warning) =>
    set((s) => ({
      warningsByGroup: {
        ...s.warningsByGroup,
        [groupId]: [...(s.warningsByGroup[groupId] ?? []), warning],
      },
    })),

  clearWarnings: (groupId) =>
    set((s) => ({
      warningsByGroup: { ...s.warningsByGroup, [groupId]: [] },
    })),

  pushToolActivity: (groupId, activity) =>
    set((s) => {
      const existing = s.toolActivityByGroup[groupId] ?? []
      const index = existing.findIndex((item) => item.id === activity.id)
      const next =
        index === -1
          ? [...existing, activity]
          : existing.map((item, itemIndex) =>
              itemIndex === index ? { ...item, ...activity } : item,
            )
      return {
        toolActivityByGroup: {
          ...s.toolActivityByGroup,
          [groupId]: next.slice(-8),
        },
      }
    }),

  clearToolActivity: (groupId) =>
    set((s) => ({
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  appendToMessage: (groupId, messageId, delta) =>
    set((s) => {
      const list = s.byGroup[groupId] ?? []
      const next = list.map((m) =>
        m.id === messageId ? { ...m, content: (m.content ?? '') + delta } : m,
      )
      return { byGroup: { ...s.byGroup, [groupId]: next } }
    }),

  replaceMessage: (groupId, message) =>
    set((s) => {
      const list = s.byGroup[groupId] ?? []
      const next = list.map((m) => (m.id === message.id ? message : m))
      return { byGroup: { ...s.byGroup, [groupId]: next } }
    }),

  startResume: (messageId) =>
    set((s) => {
      const next = new Set(s.resumingMessageIds)
      next.add(messageId)
      return { resumingMessageIds: next }
    }),

  endResume: (messageId) =>
    set((s) => {
      const next = new Set(s.resumingMessageIds)
      next.delete(messageId)
      return { resumingMessageIds: next }
    }),
}))
