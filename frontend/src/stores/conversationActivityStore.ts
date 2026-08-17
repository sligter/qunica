/**
 * Which conversations are busy right now.
 *
 * The message store already holds everything a *rendered* conversation needs,
 * but it is keyed by the bucket a view reads through — a task's thread id, or
 * the conversation id for the main thread — so nothing there answers "is group
 * X still working?" from a screen that is not group X. The sidebar and the task
 * switcher both ask exactly that, and the reply notification needs the same
 * answer plus the titles to name the conversation it is talking about.
 *
 * A run is registered when a send or resume opens its SSE stream and removed
 * when that stream reaches a terminal event. Streams outlive the view that
 * started them — navigating away does not abort them — so a run stays counted
 * until the server says it is done.
 *
 * Two outcomes outlive their run, because the stream closes while the work is
 * still the user's to finish: a turn paused on a question, and a turn that
 * failed. Both stay on the conversation until they are answered or seen.
 */

import { create } from 'zustand'

import type { ConversationScope } from '@/hooks/useGroupMessages'

/**
 * What a conversation is doing, worst-first.
 *
 * `waiting` outranks `running` because it is the only one the user can act on,
 * and both it and `failed` outlive the run that produced them: a pause that
 * cleared itself the moment the stream closed would never reach a list the
 * user was not looking at.
 */
export type ConversationActivityStatus = 'waiting' | 'running' | 'failed'

export type ConversationRunOutcome = 'completed' | 'failed' | 'cancelled'

export interface ConversationActivityRun {
  id: string
  conversation_id: string
  thread_id: string | null
  scope: ConversationScope
  conversation_title: string | null
  thread_title: string | null
  status: 'running' | 'waiting'
  /** Set once this run has produced a notification, so it produces only one. */
  announced: boolean
  started_at: string
  updated_at: string
}

/** A terminal state the conversation is still carrying after its stream ended. */
export interface ConversationPendingState {
  conversation_id: string
  thread_id: string | null
  status: 'waiting' | 'failed'
  message: string | null
  updated_at: string
}

export interface ViewedConversation {
  conversation_id: string
  thread_id: string | null
}

export interface ConversationTitles {
  conversation: string | null
  thread: string | null
}

export interface StartConversationRunInput {
  /** The stream id the backend echoes back, or the resumed message id. */
  id: string
  conversationId: string
  threadId?: string | null
  scope: ConversationScope
}

interface ConversationActivityState {
  runs: Record<string, ConversationActivityRun>
  /** Keyed by [`conversationActivityKey`]. */
  pending: Record<string, ConversationPendingState>
  /**
   * Plain-text names, keyed by [`conversationActivityKey`].
   *
   * A notification fires from outside React and has no route back to the tree
   * that knows what a conversation is called, so the chat view leaves the name
   * here while it is mounted and every run started from it reads it back.
   */
  titles: Record<string, ConversationTitles>
  /** The conversation on screen, so a notification can stay quiet about it. */
  viewed: ViewedConversation | null

  registerConversationTitles: (
    conversationId: string,
    threadId: string | null | undefined,
    titles: ConversationTitles,
  ) => void
  startRun: (input: StartConversationRunInput) => void
  /**
   * Move a run to "paused on the user".
   *
   * Returns the run when this is the transition — the caller announces it, and
   * the `announced` flag it leaves behind stops the terminal event announcing
   * the same pause a second time.
   */
  markRunWaiting: (id: string) => ConversationActivityRun | null
  /** Retire a run, returning it so the caller can announce how it ended. */
  finishRun: (
    id: string,
    outcome: ConversationRunOutcome,
    message?: string,
  ) => ConversationActivityRun | null
  /**
   * Drop a failure the user has now seen.
   *
   * A pause is left alone: opening the conversation does not answer the
   * question, and the next message clears it through `startRun`.
   */
  clearFailure: (conversationId: string, threadId?: string | null) => void
  setViewedConversation: (conversationId: string, threadId?: string | null) => void
  clearViewedConversation: (conversationId: string) => void
}

export function conversationActivityKey(
  conversationId: string,
  threadId?: string | null,
): string {
  return `${conversationId}:${threadId ?? ''}`
}

function nowIso(): string {
  return new Date().toISOString()
}

function statusOf(
  runs: ConversationActivityRun[],
  pending: ConversationPendingState[],
): ConversationActivityStatus | null {
  if (runs.some((run) => run.status === 'waiting')) return 'waiting'
  if (pending.some((state) => state.status === 'waiting')) return 'waiting'
  if (runs.length > 0) return 'running'
  return pending.length > 0 ? 'failed' : null
}

/** The status of a conversation as a whole, across every task thread in it. */
export function selectConversationStatus(
  state: ConversationActivityState,
  conversationId: string | undefined,
): ConversationActivityStatus | null {
  if (!conversationId) return null
  return statusOf(
    Object.values(state.runs).filter((run) => run.conversation_id === conversationId),
    Object.values(state.pending).filter(
      (item) => item.conversation_id === conversationId,
    ),
  )
}

/** The status of one task thread inside a conversation. */
export function selectThreadStatus(
  state: ConversationActivityState,
  conversationId: string | undefined,
  threadId: string | undefined,
): ConversationActivityStatus | null {
  if (!conversationId || !threadId) return null
  const pending = state.pending[conversationActivityKey(conversationId, threadId)]
  return statusOf(
    Object.values(state.runs).filter(
      (run) => run.conversation_id === conversationId && run.thread_id === threadId,
    ),
    pending ? [pending] : [],
  )
}

export function isConversationViewed(
  viewed: ViewedConversation | null,
  run: Pick<ConversationActivityRun, 'conversation_id' | 'thread_id'>,
): boolean {
  if (!viewed) return false
  return (
    viewed.conversation_id === run.conversation_id &&
    (viewed.thread_id ?? null) === (run.thread_id ?? null)
  )
}

export const useConversationActivityStore = create<ConversationActivityState>((set, get) => ({
  runs: {},
  pending: {},
  titles: {},
  viewed: null,

  registerConversationTitles: (conversationId, threadId, titles) =>
    set((state) => {
      const key = conversationActivityKey(conversationId, threadId)
      const existing = state.titles[key]
      if (
        existing?.conversation === titles.conversation &&
        existing.thread === titles.thread
      ) {
        return {}
      }
      return { titles: { ...state.titles, [key]: titles } }
    }),

  startRun: (input) =>
    set((state) => {
      const timestamp = nowIso()
      const existing = state.runs[input.id]
      const threadId = input.threadId ?? null
      const key = conversationActivityKey(input.conversationId, threadId)
      const titles = state.titles[key]
      // A fresh attempt supersedes the previous verdict for this thread: the
      // message being sent is the answer to the question that was pending, and
      // a failure shown next to a running spinner reads as both at once.
      const pending = { ...state.pending }
      delete pending[key]
      return {
        runs: {
          ...state.runs,
          [input.id]: {
            id: input.id,
            conversation_id: input.conversationId,
            thread_id: threadId,
            scope: input.scope,
            conversation_title: titles?.conversation ?? existing?.conversation_title ?? null,
            thread_title: titles?.thread ?? existing?.thread_title ?? null,
            status: 'running',
            announced: false,
            started_at: existing?.started_at ?? timestamp,
            updated_at: timestamp,
          },
        },
        pending,
      }
    }),

  markRunWaiting: (id) => {
    const run = get().runs[id]
    if (!run || run.status === 'waiting') return null
    const next: ConversationActivityRun = {
      ...run,
      status: 'waiting',
      announced: true,
      updated_at: nowIso(),
    }
    set((state) => ({ runs: { ...state.runs, [id]: next } }))
    return next
  },

  finishRun: (id, outcome, message) => {
    const state = get()
    const run = state.runs[id]
    if (!run) return null
    // A paused turn ends its stream while the question is still open, so the
    // pause — not the clean close that carried it — is what the list shows.
    // Cancelling takes the whole turn back, question included. A failure is
    // only carried when the user was somewhere else: on screen, the error
    // banner under the composer already said it.
    let carried: ConversationPendingState['status'] | null = null
    if (outcome === 'failed') {
      carried = isConversationViewed(state.viewed, run) ? null : 'failed'
    } else if (outcome === 'completed' && run.status === 'waiting') {
      carried = 'waiting'
    }
    set((state) => {
      const runs = { ...state.runs }
      delete runs[id]
      const key = conversationActivityKey(run.conversation_id, run.thread_id)
      const pending = { ...state.pending }
      if (carried === null) {
        delete pending[key]
      } else {
        pending[key] = {
          conversation_id: run.conversation_id,
          thread_id: run.thread_id,
          status: carried,
          message: message ?? null,
          updated_at: nowIso(),
        }
      }
      return { runs, pending }
    })
    return run
  },

  clearFailure: (conversationId, threadId) =>
    set((state) => {
      const key = conversationActivityKey(conversationId, threadId)
      if (state.pending[key]?.status !== 'failed') return {}
      const pending = { ...state.pending }
      delete pending[key]
      return { pending }
    }),

  setViewedConversation: (conversationId, threadId) =>
    set({ viewed: { conversation_id: conversationId, thread_id: threadId ?? null } }),

  clearViewedConversation: (conversationId) =>
    set((state) =>
      state.viewed?.conversation_id === conversationId ? { viewed: null } : {},
    ),
}))
