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

interface MessageState {
  byGroup: Record<string, Message[]>
  inFlightByGroup: Record<string, Record<string, StreamingBubble>>
  activeAgentByGroup: Record<string, ActiveAgent | null>
  warningsByGroup: Record<string, string[]>
  resumingMessageIds: Set<string>

  setHistory: (groupId: string, messages: Message[]) => void
  appendMessage: (groupId: string, message: Message) => void
  patchInFlight: (groupId: string, agentId: string, delta: string) => void
  finalizeInFlight: (groupId: string, message: Message) => void
  clearInFlight: (groupId: string) => void
  setActiveAgent: (groupId: string, agent: ActiveAgent) => void
  clearActiveAgent: (groupId: string) => void
  pushWarning: (groupId: string, warning: string) => void
  clearWarnings: (groupId: string) => void
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
  resumingMessageIds: new Set(),

  setHistory: (groupId, messages) =>
    set((s) => ({
      byGroup: { ...s.byGroup, [groupId]: messages },
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      warningsByGroup: { ...s.warningsByGroup, [groupId]: [] },
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
    })),

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
