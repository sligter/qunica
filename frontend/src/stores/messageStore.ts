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
  agent_id: string
  content: string
}

export interface ActiveAgent {
  agent_id: string
  display_name: string
  index: number
  total: number
}

export interface ToolActivity {
  id: string
  agent_id: string
  display_name: string
  tool_name: string
  status: 'started' | 'completed' | 'failed' | 'unavailable'
  summary?: string
}

interface MessageState {
  byGroup: Record<string, Message[]>
  inFlightByGroup: Record<string, Record<string, StreamingBubble>>
  activeAgentByGroup: Record<string, ActiveAgent | null>
  warningsByGroup: Record<string, string[]>
  toolActivityByGroup: Record<string, ToolActivity[]>
  resumingMessageIds: Set<string>

  setHistory: (groupId: string, messages: Message[]) => void
  appendMessage: (groupId: string, message: Message) => void
  patchInFlight: (groupId: string, agentId: string, delta: string) => void
  finalizeInFlight: (groupId: string, message: Message) => void
  clearInFlight: (groupId: string) => void
  clearAgentInFlight: (groupId: string, agentId: string) => void
  setActiveAgent: (groupId: string, agent: ActiveAgent) => void
  clearActiveAgent: (groupId: string) => void
  pushWarning: (groupId: string, warning: string) => void
  clearWarnings: (groupId: string) => void
  pushToolActivity: (groupId: string, activity: ToolActivity) => void
  clearToolActivity: (groupId: string) => void
  appendToMessage: (groupId: string, messageId: string, delta: string) => void
  replaceMessage: (groupId: string, message: Message) => void
  startResume: (messageId: string) => void
  endResume: (messageId: string) => void
}

export const useMessageStore = create<MessageState>((set) => ({
  byGroup: {},
  inFlightByGroup: {},
  activeAgentByGroup: {},
  warningsByGroup: {},
  toolActivityByGroup: {},
  resumingMessageIds: new Set(),

  setHistory: (groupId, messages) =>
    set((s) => ({
      byGroup: { ...s.byGroup, [groupId]: messages },
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
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

  patchInFlight: (groupId, agentId, delta) =>
    set((s) => {
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const existing = groupInFlight[agentId]
      const next: StreamingBubble = existing
        ? { ...existing, content: existing.content + delta }
        : { agent_id: agentId, content: delta }
      return {
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: { ...groupInFlight, [agentId]: next },
        },
      }
    }),

  finalizeInFlight: (groupId, message) =>
    set((s) => {
      const agentId = message.sender_id ?? ''
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remaining = { ...groupInFlight }
      delete remaining[agentId]
      return {
        byGroup: {
          ...s.byGroup,
          [groupId]: [...(s.byGroup[groupId] ?? []), message],
        },
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: remaining,
        },
      }
    }),

  clearInFlight: (groupId) =>
    set((s) => ({
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      activeAgentByGroup: { ...s.activeAgentByGroup, [groupId]: null },
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  clearAgentInFlight: (groupId, agentId) =>
    set((s) => {
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remaining = { ...groupInFlight }
      delete remaining[agentId]
      return {
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: remaining,
        },
      }
    }),

  setActiveAgent: (groupId, agent) =>
    set((s) => ({
      activeAgentByGroup: { ...s.activeAgentByGroup, [groupId]: agent },
    })),

  clearActiveAgent: (groupId) =>
    set((s) => ({
      activeAgentByGroup: { ...s.activeAgentByGroup, [groupId]: null },
    })),

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
    set((s) => ({
      toolActivityByGroup: {
        ...s.toolActivityByGroup,
        [groupId]: [...(s.toolActivityByGroup[groupId] ?? []), activity].slice(-8),
      },
    })),

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
