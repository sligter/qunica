/**
 * Queued messages store — messages parked while a reply is still streaming.
 *
 * With `reply_insert_mode: 'queue'`, a message typed mid-reply waits for the
 * current stream to end. The queue previously lived in `ConversationChatView`'s
 * local `useRef` and was explicitly cleared on conversation switch, so
 * switching groups and back silently destroyed the user's queued text. The
 * queue is per-conversation state, not per-component state: it belongs in a
 * module-level store keyed by the conversation's state id (thread id when
 * present, else the conversation id) and survives navigation.
 */

import { create } from 'zustand'

import type { MessageSendInput } from '@/types/api'

interface QueuedMessagesState {
  /** Queued inputs per conversation state key, insertion-ordered. */
  byStateId: Record<string, MessageSendInput[]>
  /** Guards one queue release against StrictMode's repeated effect setup. */
  dispatchingByStateId: Record<string, boolean>

  enqueue: (stateId: string, input: MessageSendInput[]) => void
  /** Reserve and remove the first queued input, if no release is in flight. */
  beginDispatch: (stateId: string) => MessageSendInput | undefined
  /** Finish a release, optionally returning a failed input to the queue front. */
  finishDispatch: (stateId: string, retry?: MessageSendInput) => void
  clear: (stateId: string) => void
  clearAll: () => void
}

export const useQueuedMessagesStore = create<QueuedMessagesState>((set, get) => ({
  byStateId: {},
  dispatchingByStateId: {},

  enqueue: (stateId, inputs) =>
    set((state) => ({
      byStateId: {
        ...state.byStateId,
        [stateId]: [...(state.byStateId[stateId] ?? []), ...inputs],
      },
    })),

  beginDispatch: (stateId) => {
    const state = get()
    if (state.dispatchingByStateId[stateId]) return undefined
    const current = state.byStateId[stateId] ?? []
    if (current.length === 0) return undefined
    const [next, ...rest] = current
    set((currentState) => {
      const byStateId = { ...currentState.byStateId }
      if (rest.length === 0) delete byStateId[stateId]
      else byStateId[stateId] = rest
      return {
        byStateId,
        dispatchingByStateId: {
          ...currentState.dispatchingByStateId,
          [stateId]: true,
        },
      }
    })
    return next
  },

  finishDispatch: (stateId, retry) =>
    set((state) => {
      // `clearAll` (logout / cross-window auth change) invalidates every
      // in-flight release. A stale rejection must not resurrect the previous
      // account's private message after its queue has been cleared.
      if (!state.dispatchingByStateId[stateId]) return {}
      const dispatchingByStateId = { ...state.dispatchingByStateId }
      delete dispatchingByStateId[stateId]
      if (!retry) return { dispatchingByStateId }
      return {
        dispatchingByStateId,
        byStateId: {
          ...state.byStateId,
          [stateId]: [retry, ...(state.byStateId[stateId] ?? [])],
        },
      }
    }),

  clear: (stateId) =>
    set((state) => {
      if (!(stateId in state.byStateId)) return {}
      const next = { ...state.byStateId }
      delete next[stateId]
      return { byStateId: next }
    }),

  clearAll: () => set({ byStateId: {}, dispatchingByStateId: {} }),
}))

/** Reactive count for the queue banner; non-reactive reads use the store directly. */
export function queuedCountOf(stateId: string): number {
  return useQueuedMessagesStore.getState().byStateId[stateId]?.length ?? 0
}
