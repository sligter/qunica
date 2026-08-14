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
  parseGroupTurnTrace,
  parseSchedulerStreamEvent,
  waitingForUserPayloadSchema,
} from '@/lib/api-v2/schemas'
import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type { RetryState } from '@/lib/api-v2/retry'
import type { SchedulerStreamUpdate, StreamEvent } from '@/lib/api-v2/types'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
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

export function useResumeStream(
  groupId: string | undefined,
  threadId: string | null | undefined,
  messageId: string | undefined,
) {
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
  const markStreamRunCancelled = useMessageStore((s) => s.markStreamRunCancelled)
  const reconcileSchedulerTurn = useMessageStore((s) => s.reconcileSchedulerTurn)
  const activeResume = useMessageStore((s) =>
    messageId ? s.activeResumesByMessageId[messageId] : undefined,
  )
  const qc = useQueryClient()

  const [isStreaming, setIsStreaming] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [retry, setRetry] = useState<RetryState | null>(null)
  const [retryExhausted, setRetryExhausted] = useState(false)
  const ctrlRef = useRef<AbortController | null>(null)
  const streamIdRef = useRef<string | null>(null)

  const finish = useCallback(() => {
    setIsStreaming(false)
    setRetry(null)
    ctrlRef.current = null
    streamIdRef.current = null
    if (messageId) endResume(messageId)
    if (groupId) {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'messages'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
    }
  }, [endResume, groupId, messageId, qc])

  const cancel = useCallback(async () => {
    const streamId = streamIdRef.current
    const state = useMessageStore.getState()
    const turnId =
      (streamId ? state.streamRunsByGroup[groupId ?? '']?.[streamId]?.turn_id : null) ??
      (groupId
        ? state.byGroup[groupId]?.find((message) => message.id === messageId)?.turn_id
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
          reconcileSchedulerTurn(groupId, trace)
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
    if (groupId && streamId && !turnId) {
      markStreamRunCancelled(groupId, [streamId])
    }
    finish()
  }, [
    finish,
    groupId,
    markStreamRunCancelled,
    messageId,
    reconcileSchedulerTurn,
    threadId,
    token,
  ])

  const resume = useCallback(() => {
    if (!groupId || !threadId || !messageId || !token || isStreaming || activeResume) return
    setError(null)
    setRetry(null)
    setRetryExhausted(false)
    setIsStreaming(true)
    startResume(groupId, messageId, cancel)

    const ctrl = openApiV2SseStream({
      url: `/api/v2/threads/${threadId}/resume`,
      body: {},
      token,
      handlers: {
        onEvent: (event) => {
          setRetry(null)
          const streamId = event.stream_id
          streamIdRef.current = streamId
          const schedulerUpdate = parseSchedulerStreamEvent(event)
          if (schedulerUpdate) {
            if (!applySchedulerEvent(groupId, streamId, schedulerUpdate)) return
            const interrupted = useMessageStore
              .getState()
              .byGroup[groupId]?.find((message) => message.id === messageId)
            if (interrupted?.reply_to_message_id) {
              linkStreamRunToUserMessage(
                groupId,
                streamId,
                interrupted.reply_to_message_id,
              )
            }
            if (isTerminalSchedulerUpdate(schedulerUpdate)) {
              void qc.invalidateQueries({
                queryKey: ['groups', groupId, 'turns', schedulerUpdate.payload.turn_id],
              })
            }
            if (schedulerUpdate.kind === 'done') {
              markStreamRunDone(groupId, streamId)
              finish()
            }
            return
          }
          if (!acceptsStreamEvent(groupId, streamId)) return
          switch (event.kind) {
            case 'token': {
              const parsed = tokenPayloadSchema.safeParse(event.payload)
              if (!parsed.success) return
              const delta = parsed.data.delta ?? parsed.data.text ?? ''
              if (delta) appendToMessage(groupId, messageId, delta)
              return
            }
            case 'agent_message': {
              const parsed = agentMessagePayloadSchema.safeParse(event.payload)
              if (!parsed.success) return
              const state = useMessageStore.getState()
              const previous = state.byGroup[groupId]?.find(
                (message) => message.id === parsed.data.message_id,
              )
              const turnId = state.streamRunsByGroup[groupId]?.[streamId]?.turn_id ?? null
              replaceMessage(
                groupId,
                buildAgentMessage(groupId, threadId, event, parsed.data, previous, turnId),
              )
              return
            }
            case 'error': {
              setError(errorMessage(event.payload))
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
              appendStreamNotice(groupId, streamId, {
                type: 'waiting_for_user',
                agent_id: payload?.agent_id,
                display_name: payload?.display_name,
                message: payload?.message ?? 'Waiting for your input',
                input_request: payload?.input_request,
              })
              const turnId = markStreamRunWaitingForUser(groupId, streamId)
              if (turnId) {
                void qc.invalidateQueries({
                  queryKey: ['groups', groupId, 'turns', turnId],
                })
              }
              return
            }
            case 'user_message':
            case 'conversation_updated':
            case 'agent_start':
            case 'reasoning':
            case 'tool_call_start':
            case 'tool_call_result':
            case 'agent_silent':
            case 'context_usage':
            case 'acp_agent_run':
            case 'silence':
            case 'warning':
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
          setRetryExhausted(
            err instanceof Error && err.name === 'SseRetryExhaustedError',
          )
          setError(err instanceof Error ? err.message : String(err))
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
    markStreamRunDone,
    markStreamRunWaitingForUser,
    qc,
    replaceMessage,
    startResume,
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
