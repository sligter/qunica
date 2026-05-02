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

interface MessageState {
  byGroup: Record<string, Message[]>
  inFlightByGroup: Record<string, Record<string, StreamingBubble>>
  warningsByGroup: Record<string, string[]>

  setHistory: (groupId: string, messages: Message[]) => void
  appendMessage: (groupId: string, message: Message) => void
  patchInFlight: (groupId: string, agentId: string, delta: string) => void
  finalizeInFlight: (groupId: string, message: Message) => void
  clearInFlight: (groupId: string) => void
  pushWarning: (groupId: string, warning: string) => void
  clearWarnings: (groupId: string) => void
}

export const useMessageStore = create<MessageState>((set) => ({
  byGroup: {},
  inFlightByGroup: {},
  warningsByGroup: {},

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
}))
