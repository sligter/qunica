/**
 * Resume a paused thread through the API v2 typed SSE contract.
 *
 * `resume()` POSTs to `/api/v2/threads/{threadId}/resume` and streams
 * continuation tokens. Each token is appended to the existing interrupted
 * message; the final `agent_message` event replaces that message locally
 * until query invalidation reconciles with the persisted row.
 */

import { useCallback, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { z } from 'zod'

import { fetchJson } from '@/lib/api-v2/client'
import {
  approvalRequiredPayloadSchema,
  parseGroupTurnTrace,
  parseSchedulerStreamEvent,
  todoItemSchema,
  waitingForUserPayloadSchema,
} from '@/lib/api-v2/schemas'
import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type { RetryState } from '@/lib/api-v2/retry'
import type { SchedulerStreamUpdate, StreamEvent } from '@/lib/api-v2/types'
import { conversationMessagesKey } from '@/hooks/useGroupMessages'
import type { ConversationScope } from '@/hooks/useGroupMessages'
import { notifyReplyOutcome } from '@/lib/replyNotifications'
import { useAuthStore } from '@/stores/authStore'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ApprovalAnswer } from '@/components/chat/ToolApprovalCard'
import type { Message } from '@/types/api'

const tokenPayloadSchema = z.object({
  delta: z.string().optional(),
  text: z.string().optional(),
})

const agentMessagePayloadSchema = z.object({
  message_id: z.string(),
  agent_id: z.string().optional(),
  sender_id: z.string().nullable().optional(),
  content: z.string().nullable().optional(),
  response_segments: z.array(z.string()).nullable().optional(),
  reasoning: z.array(z.string()).nullable().optional(),
  tool_calls: z
    .array(
      // Defaulted rather than optional so the parsed shape matches
      // `MessageToolCall`, where every field is present but nullable.
      z.object({
        tool_call_id: z.string().nullable().default(null),
        tool_name: z.string().nullable().default(null),
        status: z.string().nullable().default(null),
        args_summary: z.string().nullable().default(null),
        result_summary: z.string().nullable().default(null),
      }),
    )
    .nullable()
    .optional(),
  todos: z.array(todoItemSchema).nullable().optional(),
})

const errorPayloadSchema = z.object({
  message: z.string().optional(),
})

type AgentMessagePayload = z.infer<typeof agentMessagePayloadSchema>

function nowIso(): string {
  return new Date().toISOString()
}

function buildAgentMessage(
  groupId: string,
  threadId: string,
  event: StreamEvent,
  payload: AgentMessagePayload,
  previous: Message | undefined,
  turnId: string | null,
): Message {
  return {
    id: payload.message_id,
    group_id: groupId,
    thread_id: threadId,
    sender_type: 'agent',
    sender_id: payload.sender_id ?? payload.agent_id ?? null,
    message_type: 'text',
    content: payload.content ?? '',
    attachments: previous?.attachments ?? [],
    status: 'visible',
    refs: previous?.refs ?? null,
    context_usage: previous?.context_usage ?? null,
    response_segments: payload.response_segments ?? previous?.response_segments ?? null,
    reasoning: payload.reasoning ?? previous?.reasoning ?? null,
    tool_calls: payload.tool_calls ?? previous?.tool_calls ?? null,
    todos: payload.todos ?? previous?.todos ?? null,
    turn_id: turnId ?? previous?.turn_id ?? null,
    dispatch_id: previous?.dispatch_id ?? null,
    reply_to_message_id: previous?.reply_to_message_id ?? event.stream_id,
    turn_summary: previous?.turn_summary ?? null,
    created_at: previous?.created_at ?? nowIso(),
  }
}

function isTerminalSchedulerUpdate(update: SchedulerStreamUpdate): boolean {
  switch (update.kind) {
    case 'turn_cancelled':
    case 'turn_superseded':
    case 'turn_budget_exhausted':
    case 'turn_completed':
      return true
    case 'turn_started':
    case 'speaker_selected':
    case 'dispatch_failed':
    case 'moderator_fallback':
    case 'done':
      return false
  }
}

function errorMessage(payload: unknown): string {
  const parsed = errorPayloadSchema.safeParse(payload)
  return parsed.success ? parsed.data.message ?? 'Resume failed' : 'Resume failed'
}

/**
 * Continue the interrupted message `messageId` in the conversation `stateId`.
 *
 * `stateId` is the store bucket the conversation is read through — a task's
 * thread id, or the group id for the main conversation. The thread to resume is
 * taken from the message itself rather than from the caller: those two are the
 * same for a task and different for the main conversation, and deriving one
 * from the other used to put the streamed tokens in a bucket nothing renders.
 *
 * `scope` is the container the conversation lives in. It only reaches the query
 * keys, but a wrong one there means the history the view reads is never
 * invalidated and the resumed message stays stale until something else
 * refetches it.
 */
export function useResumeStream(
  groupId: string | undefined,
  stateId: string | undefined,
  messageId: string | undefined,
  scope: ConversationScope = 'groups',
) {
  const threadId = useMessageStore((s) =>
    stateId && messageId
      ? s.byGroup[stateId]?.find((message) => message.id === messageId)?.thread_id ?? null
      : null,
  )
  const token = useAuthStore((s) => s.token)
  const appendToMessage = useMessageStore((s) => s.appendToMessage)
  const replaceMessage = useMessageStore((s) => s.replaceMessage)
  const startResume = useMessageStore((s) => s.startResume)
  const endResume = useMessageStore((s) => s.endResume)
  const applySchedulerEvent = useMessageStore((s) => s.applySchedulerEvent)
  const linkStreamRunToUserMessage = useMessageStore((s) => s.linkStreamRunToUserMessage)
  const acceptsStreamEvent = useMessageStore((s) => s.acceptsStreamEvent)
  const markStreamRunWaitingForUser = useMessageStore((s) => s.markStreamRunWaitingForUser)
  const appendStreamNotice = useMessageStore((s) => s.appendStreamNotice)
  const markStreamRunDone = useMessageStore((s) => s.markStreamRunDone)
  const resolveStreamApproval = useMessageStore((s) => s.resolveStreamApproval)
  const markStreamRunCancelled = useMessageStore((s) => s.markStreamRunCancelled)
  const reconcileSchedulerTurn = useMessageStore((s) => s.reconcileSchedulerTurn)
  const activeResume = useMessageStore((s) =>
    messageId ? s.activeResumesByMessageId[messageId] : undefined,
  )
  const startActivityRun = useConversationActivityStore((s) => s.startRun)
  const markActivityRunWaiting = useConversationActivityStore((s) => s.markRunWaiting)
  const finishActivityRun = useConversationActivityStore((s) => s.finishRun)
  const qc = useQueryClient()

  const [isStreaming, setIsStreaming] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [retry, setRetry] = useState<RetryState | null>(null)
  const [retryExhausted, setRetryExhausted] = useState(false)
  const ctrlRef = useRef<AbortController | null>(null)
  const streamIdRef = useRef<string | null>(null)
  /** Why the resume failed, so the run reports the same reason it showed. */
  const failureRef = useRef<string | null>(null)

  // The history query is keyed by the *view's* thread, which is a thread id for
  // a task and absent for the main conversation — the same distinction `stateId`
  // encodes. Keying the invalidation off the message's own thread would miss the
  // main conversation's cache entirely and leave the resumed message stale.
  const queryThreadId = stateId && stateId !== groupId ? stateId : undefined

  const finish = useCallback(() => {
    setIsStreaming(false)
    setRetry(null)
    ctrlRef.current = null
    streamIdRef.current = null
    const failure = failureRef.current
    failureRef.current = null
    if (messageId) {
      endResume(messageId)
      const outcome = failure ? 'failed' : 'completed'
      const finishedRun = finishActivityRun(messageId, outcome, failure ?? undefined)
      if (finishedRun && !finishedRun.announced) {
        notifyReplyOutcome(finishedRun, outcome, failure ?? undefined)
      }
    }
    if (groupId) {
      void qc.invalidateQueries({
        queryKey: conversationMessagesKey(scope, groupId, queryThreadId),
      })
      // A direct chat has no agent roster query to refresh; its sole agent is
      // resolved from the chat itself.
      if (scope === 'groups') {
        void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      }
    }
  }, [endResume, finishActivityRun, groupId, messageId, qc, queryThreadId, scope])

  const cancel = useCallback(async () => {
    const streamId = streamIdRef.current
    const state = useMessageStore.getState()
    const turnId =
      (streamId && stateId ? state.streamRunsByGroup[stateId]?.[streamId]?.turn_id : null) ??
      (stateId
        ? state.byGroup[stateId]?.find((message) => message.id === messageId)?.turn_id
        : null)
    if (token && threadId) {
      try {
        if (groupId && turnId) {
          const trace = parseGroupTurnTrace(
            await fetchJson<unknown>(`/groups/${groupId}/turns/${turnId}/cancel`, {
              method: 'POST',
              token,
            }),
          )
          if (stateId) reconcileSchedulerTurn(stateId, trace)
        } else {
          await fetchJson<void>(`/threads/${threadId}/cancel`, {
            method: 'POST',
            token,
          })
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        return
      }
    }
    ctrlRef.current?.abort()
    if (stateId && streamId && !turnId) {
      markStreamRunCancelled(stateId, [streamId])
    }
    if (messageId) finishActivityRun(messageId, 'cancelled')
    finish()
  }, [
    finish,
    finishActivityRun,
    groupId,
    markStreamRunCancelled,
    messageId,
    reconcileSchedulerTurn,
    stateId,
    threadId,
    token,
  ])

  /**
   * Continue the paused thread.
   *
   * `approval` answers a tool call the turn stopped on; the runtime replays that
   * exact call rather than re-prompting the model, so the command that runs is
   * the one the card showed.
   */
  const resume = useCallback((approval?: ApprovalAnswer) => {
    if (!groupId || !stateId || !threadId || !messageId || !token || isStreaming || activeResume) return
    setError(null)
    setRetry(null)
    setRetryExhausted(false)
    setIsStreaming(true)
    failureRef.current = null
    startResume(stateId, messageId, cancel)
    startActivityRun({
      id: messageId,
      conversationId: groupId,
      threadId: queryThreadId,
      scope,
    })
    if (approval) {
      resolveStreamApproval(
        stateId,
        approval.tool_call_id,
        approval.approved ? 'approved' : 'declined',
      )
    }

    const ctrl = openApiV2SseStream({
      url: `/api/v2/threads/${threadId}/resume`,
      body: approval ? { approval } : {},
      token,
      handlers: {
        onEvent: (event) => {
          setRetry(null)
          const streamId = event.stream_id
          streamIdRef.current = streamId
          // A run is only rendered under the user message that asked for it, so
          // an unlinked resume run is an invisible one — its approval card and
          // waiting-for-input prompt would never reach the screen, leaving the
          // paused turn answerable only by pressing continue again. Linking is
          // idempotent and a no-op until the run exists, so it is safe to
          // attempt after anything that may have created one.
          const linkRun = () => {
            const interrupted = useMessageStore
              .getState()
              .byGroup[stateId]?.find((message) => message.id === messageId)
            if (interrupted?.reply_to_message_id) {
              linkStreamRunToUserMessage(
                stateId,
                streamId,
                interrupted.reply_to_message_id,
              )
            }
          }
          const schedulerUpdate = parseSchedulerStreamEvent(event)
          if (schedulerUpdate) {
            if (!applySchedulerEvent(stateId, streamId, schedulerUpdate)) return
            linkRun()
            if (isTerminalSchedulerUpdate(schedulerUpdate)) {
              void qc.invalidateQueries({
                queryKey: ['groups', groupId, 'turns', schedulerUpdate.payload.turn_id],
              })
            }
            if (schedulerUpdate.kind === 'done') {
              markStreamRunDone(stateId, streamId)
              finish()
            }
            return
          }
          if (!acceptsStreamEvent(stateId, streamId)) return
          switch (event.kind) {
            case 'token': {
              const parsed = tokenPayloadSchema.safeParse(event.payload)
              if (!parsed.success) return
              const delta = parsed.data.delta ?? parsed.data.text ?? ''
              if (delta) appendToMessage(stateId, messageId, delta)
              return
            }
            case 'agent_message': {
              const parsed = agentMessagePayloadSchema.safeParse(event.payload)
              if (!parsed.success) return
              const state = useMessageStore.getState()
              const previous = state.byGroup[stateId]?.find(
                (message) => message.id === parsed.data.message_id,
              )
              const turnId = state.streamRunsByGroup[stateId]?.[streamId]?.turn_id ?? null
              replaceMessage(
                stateId,
                buildAgentMessage(groupId, threadId, event, parsed.data, previous, turnId),
              )
              return
            }
            case 'error': {
              const message = errorMessage(event.payload)
              failureRef.current = message
              setError(message)
              finish()
              return
            }
            case 'done': {
              finish()
              return
            }
            case 'waiting_for_user': {
              // Replay the notice too, not just the run status. Reloading while
              // an agent waits would otherwise lose the question entirely.
              const parsed = waitingForUserPayloadSchema.safeParse(event.payload)
              const payload = parsed.success ? parsed.data : undefined
              appendStreamNotice(stateId, streamId, {
                type: 'waiting_for_user',
                agent_id: payload?.agent_id,
                display_name: payload?.display_name,
                message: payload?.message ?? 'Waiting for your input',
                input_request: payload?.input_request,
              })
              linkRun()
              const turnId = markStreamRunWaitingForUser(stateId, streamId)
              if (turnId) {
                void qc.invalidateQueries({
                  queryKey: ['groups', groupId, 'turns', turnId],
                })
              }
              const waitingRun = markActivityRunWaiting(messageId)
              if (waitingRun) notifyReplyOutcome(waitingRun, 'waiting')
              return
            }
            case 'approval_required': {
              // Replayed like the waiting notice: reloading mid-pause must not
              // lose the card, or the paused turn becomes unanswerable.
              const parsed = approvalRequiredPayloadSchema.safeParse(event.payload)
              if (!parsed.success) return
              appendStreamNotice(stateId, streamId, {
                type: 'approval_required',
                agent_id: parsed.data.agent_id,
                display_name: parsed.data.display_name,
                message: parsed.data.message ?? 'Approval required',
                approval_request: {
                  tool_call_id: parsed.data.tool_call_id,
                  ...parsed.data.approval_request,
                },
              })
              linkRun()
              const turnId = markStreamRunWaitingForUser(stateId, streamId)
              if (turnId) {
                void qc.invalidateQueries({
                  queryKey: ['groups', groupId, 'turns', turnId],
                })
              }
              const approvalRun = markActivityRunWaiting(messageId)
              if (approvalRun) notifyReplyOutcome(approvalRun, 'waiting')
              return
            }
            case 'user_message':
            case 'conversation_updated':
            case 'agent_start':
            case 'reasoning':
            case 'tool_call_start':
            case 'tool_call_result':
            case 'todo_update':
            case 'agent_silent':
            case 'context_usage':
            case 'acp_agent_run':
            case 'silence':
            case 'warning':
              // The resumed `agent_message` payload carries the turn structure
              // these events describe — reasoning, tool cards, the checklist —
              // so replaying them live would only duplicate it.
              return
            default: {
              const _exhaustive: never = event.kind
              return _exhaustive
            }
          }
        },
        onRetry: (attempt, delayMs) => {
          setRetry({ attempt, delayMs })
        },
        onError: (err) => {
          const message = err instanceof Error ? err.message : String(err)
          setRetryExhausted(
            err instanceof Error && err.name === 'SseRetryExhaustedError',
          )
          failureRef.current = message
          setError(message)
          finish()
        },
        onClose: () => {
          finish()
        },
      },
    })
    ctrlRef.current = ctrl
  }, [
    appendStreamNotice,
    appendToMessage,
    activeResume,
    acceptsStreamEvent,
    applySchedulerEvent,
    cancel,
    finish,
    groupId,
    isStreaming,
    linkStreamRunToUserMessage,
    messageId,
    markActivityRunWaiting,
    markStreamRunDone,
    markStreamRunWaitingForUser,
    qc,
    queryThreadId,
    replaceMessage,
    resolveStreamApproval,
    scope,
    startActivityRun,
    startResume,
    stateId,
    threadId,
    token,
  ])

  return {
    resume,
    cancel: activeResume?.cancel ?? cancel,
    isStreaming: isStreaming || Boolean(activeResume),
    error,
    retry,
    retryExhausted,
  }
}
