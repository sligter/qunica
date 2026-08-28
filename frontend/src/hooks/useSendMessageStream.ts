/**
 * Stream-send messages to a group through the API v2 typed SSE contract.
 *
 * Multiple sends may be active at once. API v2 uses a runtime `stream_id`
 * for each SSE run, while the persisted user message has its own id in the
 * `user_message` payload. Keep those ids separate so concurrent agent drafts
 * never share transient state.
 */

import { useCallback, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { z } from 'zod'

import { fetchJson } from '@/lib/api-v2/client'
import {
  approvalRequiredPayloadSchema,
  parseConversationUpdatedEvent,
  parseGroupTurnTrace,
  parseSchedulerStreamEvent,
  todoUpdatePayloadSchema,
  waitingForUserPayloadSchema,
} from '@/lib/api-v2/schemas'
import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type { RetryState } from '@/lib/api-v2/retry'
import { pendingActionFromOutput } from '@/lib/appActions'
import type {
  ConversationUpdatedPayload,
  StreamEvent,
} from '@/lib/api-v2/types'
import {
  conversationApiPath,
  conversationMessagesKey,
  conversationStateKey,
  type ConversationScope,
} from '@/hooks/useGroupMessages'
import { conversationWorkspaceFilesQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import { notifyReplyOutcome } from '@/lib/replyNotifications'
import { useAuthStore } from '@/stores/authStore'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ActiveAgent, ToolActivity, ToolActivityStatus } from '@/stores/messageStore'
import type { ContextUsage, Message, MessageSendInput } from '@/types/api'

const messageAttachmentSchema = z.object({
  id: z.string(),
  path: z.string(),
  name: z.string(),
  mime_type: z.string(),
  size: z.number(),
  kind: z.enum(['image', 'file']),
})

const userMessagePayloadSchema = z.object({
  message_id: z.string(),
  thread_id: z.string().nullable().optional(),
  content: z.string().nullable().optional(),
  attachments: z.array(messageAttachmentSchema).default([]),
})

const agentStartPayloadSchema = z.object({
  agent_id: z.string(),
  display_name: z.string().optional(),
})

const tokenPayloadSchema = z.object({
  agent_id: z.string(),
  text: z.string().optional(),
  delta: z.string().optional(),
})

const contextUsageSchema = z.object({
  input_tokens: z.number().nullable().optional(),
  output_tokens: z.number().nullable().optional(),
  total_tokens: z.number().nullable().optional(),
  context_window_tokens: z.number().nullable().optional(),
  output_reserve_tokens: z.number().nullable().optional(),
  ratio: z.number().nullable().optional(),
  source: z.string().nullable().optional(),
  updated_at: z.string().nullable().optional(),
})

const contextUsagePayloadSchema = z.object({
  agent_id: z.string().optional(),
  input_tokens: z.number().nullable().optional(),
  output_tokens: z.number().nullable().optional(),
  total_tokens: z.number().nullable().optional(),
  context_usage: contextUsageSchema.nullable().optional(),
})

const agentMessagePayloadSchema = z.object({
  message_id: z.string(),
  agent_id: z.string().optional(),
  sender_id: z.string().nullable().optional(),
  display_name: z.string().optional(),
  content: z.string().nullable().optional(),
  thread_id: z.string().nullable().optional(),
  context_usage: contextUsageSchema.nullable().optional(),
})

const toolCallPayloadSchema = z.object({
  agent_id: z.string().optional(),
  display_name: z.string().optional(),
  tool_call_id: z.string().optional(),
  tool_name: z.string().optional(),
  status: z.string().optional(),
  args_summary: z.string().optional(),
  result_summary: z.string().optional(),
  // Raw tool output. Only read to recover a staged app action; every other
  // tool's output is already summarized into `result_summary`.
  output: z.string().optional(),
})

const acpAgentRunPayloadSchema = z.object({
  run_id: z.string(),
  agent_id: z.string(),
  display_name: z.string(),
  adapter: z.string().optional(),
  status: z.string().optional(),
  cwd: z.string().optional(),
  exit_code: z.number().nullable().optional(),
  summary: z.string().optional(),
})

const messagePayloadSchema = z.object({
  message: z.string().optional(),
})

const warningPayloadSchema = z.union([z.string(), messagePayloadSchema])

type UserMessagePayload = z.infer<typeof userMessagePayloadSchema>
type AgentMessagePayload = z.infer<typeof agentMessagePayloadSchema>
type ContextUsagePayload = z.infer<typeof contextUsagePayloadSchema>
type ContextUsageInput = z.infer<typeof contextUsageSchema>

function isToolActivityStatus(status: unknown): status is ToolActivityStatus {
  switch (status) {
    case 'started':
    case 'completed':
    case 'failed':
    case 'unavailable':
    case 'setup_required':
    case 'workspace_required':
    case 'input_required':
    case 'approval_required':
      return true
    default:
      return false
  }
}

function normalizeToolStatus(status: unknown): ToolActivityStatus {
  return isToolActivityStatus(status) ? status : 'unavailable'
}

function externalRunStatus(status: string | undefined): ToolActivityStatus {
  if (status === 'running') return 'started'
  if (status === 'completed') return 'completed'
  return 'failed'
}

function requestId(): string {
  return crypto.randomUUID()
}

function nowIso(): string {
  return new Date().toISOString()
}

function normalizeContextUsage(input: ContextUsageInput | null | undefined): ContextUsage | null {
  if (!input) return null
  return {
    input_tokens: input.input_tokens ?? null,
    output_tokens: input.output_tokens ?? null,
    total_tokens: input.total_tokens ?? null,
    context_window_tokens: input.context_window_tokens ?? null,
    output_reserve_tokens: input.output_reserve_tokens ?? null,
    ratio: input.ratio ?? null,
    source: input.source ?? null,
    updated_at: input.updated_at ?? null,
  }
}

function contextUsageFromPayload(payload: ContextUsagePayload): ContextUsage | null {
  const nested = normalizeContextUsage(payload.context_usage)
  if (nested) return nested
  const hasUsage =
    payload.input_tokens !== undefined ||
    payload.output_tokens !== undefined ||
    payload.total_tokens !== undefined
  if (!hasUsage) return null
  return {
    input_tokens: payload.input_tokens ?? null,
    output_tokens: payload.output_tokens ?? null,
    total_tokens: payload.total_tokens ?? null,
    context_window_tokens: null,
    output_reserve_tokens: null,
    ratio: null,
    source: null,
    updated_at: null,
  }
}

function messageFromPayload(payload: unknown, fallback: string): string {
  const parsed = warningPayloadSchema.safeParse(payload)
  if (!parsed.success) return fallback
  if (typeof parsed.data === 'string') return parsed.data
  return parsed.data.message ?? fallback
}

function buildUserMessage(
  groupId: string,
  payload: UserMessagePayload,
  senderId: string | null,
): Message {
  return {
    id: payload.message_id,
    group_id: groupId,
    thread_id: payload.thread_id ?? null,
    sender_type: 'user',
    sender_id: senderId,
    message_type: 'text',
    content: payload.content ?? '',
    attachments: payload.attachments,
    status: 'visible',
    refs: null,
    context_usage: null,
    turn_id: null,
    dispatch_id: null,
    reply_to_message_id: null,
    turn_summary: null,
    created_at: nowIso(),
  }
}

/**
 * The local echo shown while the server persists the message.
 *
 * Attachments stay empty: only their workspace paths are known here, and the
 * acknowledgement that lands moments later carries the names, types and sizes
 * the attachment list needs.
 */
function buildOptimisticUserMessage(
  groupId: string,
  requestId: string,
  input: string | MessageSendInput,
  threadId: string | undefined,
  senderId: string | null,
): Message {
  return {
    id: `local:${requestId}`,
    group_id: groupId,
    thread_id: threadId ?? null,
    sender_type: 'user',
    sender_id: senderId,
    message_type: 'text',
    content: (typeof input === 'string' ? input : input.content) ?? '',
    attachments: [],
    status: 'visible',
    refs: null,
    context_usage: null,
    turn_id: null,
    dispatch_id: null,
    reply_to_message_id: null,
    turn_summary: null,
    created_at: nowIso(),
  }
}

function buildAgentMessage(
  groupId: string,
  event: StreamEvent,
  payload: AgentMessagePayload,
  turnId: string | null,
): Message {
  return {
    id: payload.message_id,
    group_id: groupId,
    thread_id: payload.thread_id ?? null,
    sender_type: 'agent',
    sender_id: payload.sender_id ?? payload.agent_id ?? null,
    message_type: 'text',
    content: payload.content ?? '',
    attachments: [],
    status: 'visible',
    refs: null,
    context_usage: normalizeContextUsage(payload.context_usage),
    turn_id: turnId,
    dispatch_id: null,
    reply_to_message_id: event.stream_id,
    turn_summary: null,
    created_at: nowIso(),
  }
}

interface PendingCancellation {
  requestIds: Set<string>
  completing: boolean
  promise: Promise<void>
  resolve: () => void
}

interface PendingSendAcknowledgement {
  promise: Promise<void>
  resolve: () => void
  reject: (error: Error) => void
}

function asError(error: unknown, fallback: string): Error {
  if (error instanceof Error) return error
  const message = String(error)
  return new Error(message || fallback)
}

function createPendingSendAcknowledgement(): PendingSendAcknowledgement {
  let resolve!: () => void
  let reject!: (error: Error) => void
  const promise = new Promise<void>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

interface SendMessageStreamOptions {
  scope?: ConversationScope
  threadId?: string
  onConversationUpdated?: (payload: ConversationUpdatedPayload) => void
}

export function useSendMessageStream(
  groupId: string | undefined,
  options: SendMessageStreamOptions = {},
) {
  const scope = options.scope ?? 'groups'
  const threadId = options.threadId
  const stateKey = conversationStateKey(groupId, threadId)
  const onConversationUpdated = options.onConversationUpdated
  const token = useAuthStore((s) => s.token)
  const currentUserId = useAuthStore((s) => s.user?.id ?? null)
  const appendMessage = useMessageStore((s) => s.appendMessage)
  const startOptimisticUserMessage = useMessageStore((s) => s.startOptimisticUserMessage)
  const settleOptimisticUserMessage = useMessageStore((s) => s.settleOptimisticUserMessage)
  const dropOptimisticUserMessage = useMessageStore((s) => s.dropOptimisticUserMessage)
  const patchInFlight = useMessageStore((s) => s.patchInFlight)
  const finalizeInFlight = useMessageStore((s) => s.finalizeInFlight)
  const clearInFlight = useMessageStore((s) => s.clearInFlight)
  const clearStreamInFlight = useMessageStore((s) => s.clearStreamInFlight)
  const clearAgentInFlight = useMessageStore((s) => s.clearAgentInFlight)
  const setActiveAgent = useMessageStore((s) => s.setActiveAgent)
  const clearActiveAgent = useMessageStore((s) => s.clearActiveAgent)
  const pushWarning = useMessageStore((s) => s.pushWarning)
  const clearWarnings = useMessageStore((s) => s.clearWarnings)
  const pushToolActivity = useMessageStore((s) => s.pushToolActivity)
  const clearToolActivity = useMessageStore((s) => s.clearToolActivity)
  const startStreamRun = useMessageStore((s) => s.startStreamRun)
  const addStreamAgentStart = useMessageStore((s) => s.addStreamAgentStart)
  const setStreamAgentContextUsage = useMessageStore((s) => s.setStreamAgentContextUsage)
  const patchStreamDraft = useMessageStore((s) => s.patchStreamDraft)
  const patchStreamReasoning = useMessageStore((s) => s.patchStreamReasoning)
  const clearStreamingStreamDraft = useMessageStore((s) => s.clearStreamingStreamDraft)
  const finalizeStreamDraft = useMessageStore((s) => s.finalizeStreamDraft)
  const upsertStreamTool = useMessageStore((s) => s.upsertStreamTool)
  const upsertStreamTodos = useMessageStore((s) => s.upsertStreamTodos)
  const upsertStreamExternalRun = useMessageStore((s) => s.upsertStreamExternalRun)
  const appendStreamNotice = useMessageStore((s) => s.appendStreamNotice)
  const applySchedulerEvent = useMessageStore((s) => s.applySchedulerEvent)
  const acceptsStreamEvent = useMessageStore((s) => s.acceptsStreamEvent)
  const markStreamRunWaitingForUser = useMessageStore((s) => s.markStreamRunWaitingForUser)
  const reconcileSchedulerTurn = useMessageStore((s) => s.reconcileSchedulerTurn)
  const markStreamRunDone = useMessageStore((s) => s.markStreamRunDone)
  const markStreamRunError = useMessageStore((s) => s.markStreamRunError)
  const activeSend = useMessageStore((s) =>
    stateKey ? s.activeSendsByGroup[stateKey] : undefined,
  )
  const startSend = useMessageStore((s) => s.startSend)
  const endSend = useMessageStore((s) => s.endSend)
  const startActivityRun = useConversationActivityStore((s) => s.startRun)
  const markActivityRunWaiting = useConversationActivityStore((s) => s.markRunWaiting)
  const finishActivityRun = useConversationActivityStore((s) => s.finishRun)
  const qc = useQueryClient()

  const [activeStreamCount, setActiveStreamCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [retry, setRetry] = useState<RetryState | null>(null)
  const [retryExhausted, setRetryExhausted] = useState(false)
  const streamsRef = useRef<Map<string, AbortController>>(new Map())
  const retriesRef = useRef<Map<string, RetryState>>(new Map())
  const streamIdsRef = useRef<Map<string, string>>(new Map())
  const erroredStreamIdsRef = useRef<Set<string>>(new Set())
  /** Why a send failed, kept per request so the run can report it once. */
  const sendFailuresRef = useRef<Map<string, string>>(new Map())
  const agentNamesRef = useRef<Map<string, string>>(new Map())
  const schedulerTurnByRequestRef = useRef<Map<string, string>>(new Map())
  const pendingSendAcknowledgementsRef = useRef<Map<string, PendingSendAcknowledgement>>(
    new Map(),
  )
  const optimisticMessageIdsRef = useRef<Map<string, string>>(new Map())
  const pendingCancellationRef = useRef<PendingCancellation | null>(null)

  const acknowledgeSend = useCallback((id: string) => {
    const acknowledgement = pendingSendAcknowledgementsRef.current.get(id)
    if (!acknowledgement) return
    pendingSendAcknowledgementsRef.current.delete(id)
    acknowledgement.resolve()
  }, [])

  const rejectSendBeforeAcknowledgement = useCallback((id: string, error: unknown) => {
    // Take the local echo back with the rejection: the message never reached
    // the conversation, and leaving it on screen would claim otherwise.
    const optimisticId = optimisticMessageIdsRef.current.get(id)
    if (optimisticId) {
      optimisticMessageIdsRef.current.delete(id)
      const storeId = stateKey ?? groupId
      if (storeId) dropOptimisticUserMessage(storeId, optimisticId)
    }
    const acknowledgement = pendingSendAcknowledgementsRef.current.get(id)
    if (!acknowledgement) return
    pendingSendAcknowledgementsRef.current.delete(id)
    acknowledgement.reject(asError(
      error,
      'Message stream ended before the user message was acknowledged',
    ))
  }, [dropOptimisticUserMessage, groupId, stateKey])

  const refreshActiveCount = useCallback(() => {
    setActiveStreamCount(streamsRef.current.size)
  }, [])

  const refreshRetry = useCallback(() => {
    setRetry(
      [...retriesRef.current.values()].sort((a, b) => b.attempt - a.attempt)[0] ?? null,
    )
  }, [])

  const clearRetry = useCallback((id: string) => {
    if (retriesRef.current.delete(id)) refreshRetry()
  }, [refreshRetry])

  const invalidate = useCallback(() => {
    if (!groupId) return
    void qc.invalidateQueries({
      queryKey: conversationMessagesKey(scope, groupId, threadId),
    })
    void qc.invalidateQueries({ queryKey: conversationWorkspaceFilesQueryKey(scope, groupId) })
    if (scope === 'groups') {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'threads'] })
    }
  }, [groupId, qc, scope, threadId])

  const invalidateTurn = useCallback(
    (turnId: string) => {
      if (!groupId) return
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'turns', turnId] })
    },
    [groupId, qc],
  )

  const completePendingCancellation = useCallback(async () => {
    const pending = pendingCancellationRef.current
    if (!pending || pending.completing) return

    const activeRequestIds = Array.from(pending.requestIds).filter((id) =>
      streamsRef.current.has(id),
    )
    for (const id of activeRequestIds) {
      if (!schedulerTurnByRequestRef.current.has(id)) return
    }

    pending.completing = true
    const turnIds = new Set(
      activeRequestIds
        .map((id) => schedulerTurnByRequestRef.current.get(id))
        .filter((turnId): turnId is string => Boolean(turnId)),
    )

    if (turnIds.size > 0 && !token) {
      setError('Authentication is required to cancel this turn')
      pendingCancellationRef.current = null
      pending.resolve()
      return
    }

    try {
      if (groupId && token) {
        const traces = await Promise.all(
          Array.from(turnIds, (turnId) =>
            fetchJson<unknown>(`/groups/${groupId}/turns/${turnId}/cancel`, {
              method: 'POST',
              token,
            }).then(parseGroupTurnTrace),
          ),
        )
        for (const trace of traces) {
          if (stateKey) reconcileSchedulerTurn(stateKey, trace)
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      pendingCancellationRef.current = null
      pending.resolve()
      return
    }
    if (pendingCancellationRef.current !== pending) return

    const streamIds = activeRequestIds
      .map((id) => streamIdsRef.current.get(id))
      .filter((streamId): streamId is string => Boolean(streamId))
    for (const id of activeRequestIds) {
      rejectSendBeforeAcknowledgement(
        id,
        new Error('Message send was cancelled before acknowledgement'),
      )
      finishActivityRun(id, 'cancelled')
      sendFailuresRef.current.delete(id)
      streamsRef.current.get(id)?.abort()
      streamsRef.current.delete(id)
      clearRetry(id)
      streamIdsRef.current.delete(id)
      schedulerTurnByRequestRef.current.delete(id)
    }
    erroredStreamIdsRef.current.clear()
    agentNamesRef.current.clear()
    refreshActiveCount()

    if (stateKey) {
      if (streamIds.length > 0) {
        for (const streamId of streamIds) {
          clearStreamInFlight(stateKey, streamId)
          clearActiveAgent(stateKey, undefined, streamId)
        }
      } else {
        clearInFlight(stateKey)
      }
      if (streamsRef.current.size === 0) endSend(stateKey)
    }
    pendingCancellationRef.current = null
    setError(null)
    pending.resolve()
    window.setTimeout(invalidate, 700)
  }, [
    clearActiveAgent,
    clearInFlight,
    clearRetry,
    clearStreamInFlight,
    endSend,
    finishActivityRun,
    groupId,
    invalidate,
    reconcileSchedulerTurn,
    rejectSendBeforeAcknowledgement,
    refreshActiveCount,
    stateKey,
    token,
  ])

  const cancelOwned = useCallback((): Promise<void> => {
    const existing = pendingCancellationRef.current
    if (existing) return existing.promise

    let resolve!: () => void
    const promise = new Promise<void>((next) => {
      resolve = next
    })
    const pending: PendingCancellation = {
      requestIds: new Set(streamsRef.current.keys()),
      completing: false,
      promise,
      resolve,
    }
    pendingCancellationRef.current = pending
    void completePendingCancellation()
    return promise
  }, [completePendingCancellation])

  const finishStream = useCallback(
    (id: string, streamId?: string | null) => {
      rejectSendBeforeAcknowledgement(
        id,
        new Error('Message stream ended before the user message was acknowledged'),
      )
      const failure = sendFailuresRef.current.get(id)
      sendFailuresRef.current.delete(id)
      const finishedRun = finishActivityRun(id, failure ? 'failed' : 'completed', failure)
      if (finishedRun && !finishedRun.announced) {
        notifyReplyOutcome(finishedRun, failure ? 'failed' : 'completed', failure)
      }
      streamsRef.current.delete(id)
      clearRetry(id)
      const resolvedStreamId = streamId ?? streamIdsRef.current.get(id)
      if (resolvedStreamId && stateKey) {
        clearActiveAgent(stateKey, undefined, resolvedStreamId)
      }
      streamIdsRef.current.delete(id)
      schedulerTurnByRequestRef.current.delete(id)
      if (resolvedStreamId) {
        erroredStreamIdsRef.current.delete(resolvedStreamId)
        for (const key of Array.from(agentNamesRef.current.keys())) {
          if (key.startsWith(`${resolvedStreamId}:`)) {
            agentNamesRef.current.delete(key)
          }
        }
      }
      refreshActiveCount()
      if (stateKey && streamsRef.current.size === 0) {
        clearActiveAgent(stateKey)
        endSend(stateKey)
      }
      invalidate()
      void completePendingCancellation()
    },
    [
      clearActiveAgent,
      clearRetry,
      completePendingCancellation,
      endSend,
      finishActivityRun,
      invalidate,
      rejectSendBeforeAcknowledgement,
      refreshActiveCount,
      stateKey,
    ],
  )

  const send = useCallback(
    (input: string | MessageSendInput): Promise<void> => {
      if (!groupId || !token) {
        const message = 'Authentication and a conversation are required to send a message'
        setError(message)
        return Promise.reject(new Error(message))
      }
      if (pendingCancellationRef.current) {
        const message = 'Cancellation is in progress'
        setError(message)
        return Promise.reject(new Error(message))
      }
      const storeId = stateKey ?? groupId
      let id: string
      try {
        id = requestId()
      } catch (error) {
        const startupError = asError(error, 'Unable to start the message stream')
        setError(startupError.message)
        return Promise.reject(startupError)
      }
      const acknowledgement = createPendingSendAcknowledgement()
      pendingSendAcknowledgementsRef.current.set(id, acknowledgement)
      setError(null)
      setRetryExhausted(false)
      clearWarnings(storeId)
      clearToolActivity(storeId)
      // The backend reuses `client_request_id` as the stream id, so the run can
      // open here rather than waiting for the first event to name it.
      const optimisticMessage = buildOptimisticUserMessage(
        groupId,
        id,
        input,
        threadId,
        currentUserId,
      )
      optimisticMessageIdsRef.current.set(id, optimisticMessage.id)
      startOptimisticUserMessage(storeId, id, optimisticMessage)
      sendFailuresRef.current.delete(id)
      startActivityRun({ id, conversationId: groupId, threadId, scope })
      const message = {
        ...(typeof input === 'string' ? { content: input, attachments: [] } : input),
        client_request_id: id,
        ...(threadId ? { thread_id: threadId } : {}),
      }

      try {
        const ctrl = openApiV2SseStream({
          url: `/api/v2${conversationApiPath(scope, groupId)}/messages/stream`,
          body: message,
          token,
          handlers: {
          onOpen: () => clearRetry(id),
          onEvent: (event) => {
            clearRetry(id)
            const streamId = event.stream_id
            streamIdsRef.current.set(id, streamId)
            const schedulerUpdate = parseSchedulerStreamEvent(event)
            if (schedulerUpdate) {
              schedulerTurnByRequestRef.current.set(id, schedulerUpdate.payload.turn_id)
              void completePendingCancellation()
              if (!applySchedulerEvent(storeId, streamId, schedulerUpdate)) return
              invalidateTurn(schedulerUpdate.payload.turn_id)
              if (schedulerUpdate.kind === 'done') {
                if (!erroredStreamIdsRef.current.has(streamId)) {
                  markStreamRunDone(storeId, streamId)
                }
                finishStream(id, streamId)
                window.setTimeout(() => clearToolActivity(storeId), 4_000)
              }
              return
            }
            if (event.kind === 'user_message') {
              const parsed = userMessagePayloadSchema.safeParse(event.payload)
              if (parsed.success) {
                acknowledgeSend(id)
              }
            }
            if (event.kind === 'error') {
              rejectSendBeforeAcknowledgement(
                id,
                new Error(messageFromPayload(event.payload, 'Stream failed')),
              )
            }
            if (!acceptsStreamEvent(storeId, streamId)) return
            const agentDisplayName = (agentId: string | undefined, fallback?: string) => {
              if (fallback) return fallback
              if (agentId) {
                return agentNamesRef.current.get(`${streamId}:${agentId}`) ?? 'Agent'
              }
              return 'Agent'
            }

            switch (event.kind) {
              case 'user_message': {
                const parsed = userMessagePayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const msg = buildUserMessage(groupId, parsed.data, currentUserId)
                const optimisticId = optimisticMessageIdsRef.current.get(id)
                if (optimisticId) {
                  optimisticMessageIdsRef.current.delete(id)
                  settleOptimisticUserMessage(storeId, optimisticId, msg)
                } else {
                  appendMessage(storeId, msg)
                }
                startStreamRun(storeId, streamId, msg)
                return
              }
              case 'agent_start': {
                const parsed = agentStartPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const displayName = parsed.data.display_name ?? 'Agent'
                agentNamesRef.current.set(`${streamId}:${parsed.data.agent_id}`, displayName)
                const info: ActiveAgent = {
                  agent_id: parsed.data.agent_id,
                  display_name: displayName,
                  index: 0,
                  total: 0,
                  stream_id: streamId,
                }
                addStreamAgentStart(storeId, streamId, info)
                setActiveAgent(storeId, info)
                return
              }
              case 'context_usage': {
                const parsed = contextUsagePayloadSchema.safeParse(event.payload)
                if (!parsed.success || !parsed.data.agent_id) return
                const usage = contextUsageFromPayload(parsed.data)
                if (usage) {
                  setStreamAgentContextUsage(storeId, streamId, parsed.data.agent_id, usage)
                }
                return
              }
              case 'token': {
                const parsed = tokenPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const delta = parsed.data.delta ?? parsed.data.text ?? ''
                if (!delta) return
                patchStreamDraft(
                  storeId,
                  streamId,
                  parsed.data.agent_id,
                  delta,
                  agentDisplayName(parsed.data.agent_id),
                )
                patchInFlight(storeId, parsed.data.agent_id, delta, streamId)
                return
              }
              case 'reasoning': {
                const parsed = tokenPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const delta = parsed.data.delta ?? parsed.data.text ?? ''
                if (!delta) return
                patchStreamReasoning(
                  storeId,
                  streamId,
                  parsed.data.agent_id,
                  delta,
                  agentDisplayName(parsed.data.agent_id),
                )
                return
              }
              case 'agent_message': {
                const parsed = agentMessagePayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const turnId = useMessageStore.getState().streamRunsByGroup[storeId]?.[streamId]
                  ?.turn_id ?? null
                const msg = buildAgentMessage(groupId, event, parsed.data, turnId)
                finalizeStreamDraft(
                  storeId,
                  streamId,
                  msg,
                  agentDisplayName(parsed.data.agent_id, parsed.data.display_name),
                )
                finalizeInFlight(storeId, msg)
                void qc.invalidateQueries({ queryKey: conversationWorkspaceFilesQueryKey(scope, groupId) })
                void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
                return
              }
              case 'agent_silent': {
                const parsed = agentStartPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                clearStreamingStreamDraft(storeId, streamId, parsed.data.agent_id)
                appendStreamNotice(storeId, streamId, {
                  type: 'agent_silent',
                  agent_id: parsed.data.agent_id,
                  display_name: agentDisplayName(
                    parsed.data.agent_id,
                    parsed.data.display_name,
                  ),
                  message: 'No visible reply',
                })
                clearAgentInFlight(storeId, parsed.data.agent_id, streamId)
                clearActiveAgent(storeId, parsed.data.agent_id, streamId)
                return
              }
              case 'tool_call_start':
              case 'tool_call_result': {
                const parsed = toolCallPayloadSchema.safeParse(event.payload)
                if (!parsed.success || !parsed.data.tool_call_id) return
                const status = normalizeToolStatus(parsed.data.status)
                const activity: ToolActivity = {
                  id: parsed.data.tool_call_id,
                  agent_id: parsed.data.agent_id ?? 'unknown-agent',
                  display_name: agentDisplayName(
                    parsed.data.agent_id,
                    parsed.data.display_name,
                  ),
                  tool_name: parsed.data.tool_name ?? 'Unknown tool',
                  status,
                  args_summary: parsed.data.args_summary,
                  result_summary: parsed.data.result_summary,
                  pending_action:
                    pendingActionFromOutput(parsed.data.output) ?? undefined,
                }
                pushToolActivity(storeId, activity)
                upsertStreamTool(storeId, streamId, activity)
                if (event.kind === 'tool_call_result') {
                  void qc.invalidateQueries({ queryKey: conversationWorkspaceFilesQueryKey(scope, groupId) })
                }
                return
              }
              case 'acp_agent_run': {
                const parsed = acpAgentRunPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const displayName = agentDisplayName(
                  parsed.data.agent_id,
                  parsed.data.display_name,
                )
                pushToolActivity(storeId, {
                  id: parsed.data.run_id,
                  agent_id: parsed.data.agent_id,
                  display_name: displayName,
                  tool_name: `External CLI: ${parsed.data.adapter ?? 'unknown'}`,
                  status: externalRunStatus(parsed.data.status),
                  args_summary: parsed.data.cwd,
                  result_summary: parsed.data.summary,
                })
                upsertStreamExternalRun(storeId, streamId, {
                  run_id: parsed.data.run_id,
                  agent_id: parsed.data.agent_id,
                  display_name: displayName,
                  adapter: parsed.data.adapter,
                  status: parsed.data.status,
                  cwd: parsed.data.cwd,
                  exit_code: parsed.data.exit_code ?? undefined,
                  summary: parsed.data.summary,
                })
                if (parsed.data.status && parsed.data.status !== 'running') {
                  void qc.invalidateQueries({ queryKey: conversationWorkspaceFilesQueryKey(scope, groupId) })
                }
                return
              }
              case 'silence': {
                pushWarning(storeId, 'No one replied')
                appendStreamNotice(storeId, streamId, {
                  type: 'warning',
                  message: 'No one replied',
                })
                return
              }
              case 'waiting_for_user': {
                const parsed = waitingForUserPayloadSchema.safeParse(event.payload)
                const payload = parsed.success ? parsed.data : undefined
                const message = payload?.message ?? 'Waiting for your input'
                appendStreamNotice(storeId, streamId, {
                  type: 'waiting_for_user',
                  agent_id: payload?.agent_id,
                  display_name: agentDisplayName(payload?.agent_id, payload?.display_name),
                  message,
                  input_request: payload?.input_request,
                })
                pushWarning(storeId, message)
                const turnId = markStreamRunWaitingForUser(storeId, streamId)
                if (turnId) invalidateTurn(turnId)
                const waitingRun = markActivityRunWaiting(id)
                if (waitingRun) notifyReplyOutcome(waitingRun, 'waiting')
                return
              }
              case 'approval_required': {
                // A gated tool call stopped the turn. The card is a durable
                // notice for the same reason the waiting one is: a reload
                // mid-pause must not lose the only way to answer it.
                const parsed = approvalRequiredPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const message = parsed.data.message ?? 'Approval required'
                appendStreamNotice(storeId, streamId, {
                  type: 'approval_required',
                  agent_id: parsed.data.agent_id,
                  display_name: agentDisplayName(
                    parsed.data.agent_id,
                    parsed.data.display_name,
                  ),
                  message,
                  approval_request: {
                    tool_call_id: parsed.data.tool_call_id,
                    ...parsed.data.approval_request,
                  },
                })
                const turnId = markStreamRunWaitingForUser(storeId, streamId)
                if (turnId) invalidateTurn(turnId)
                const approvalRun = markActivityRunWaiting(id)
                if (approvalRun) notifyReplyOutcome(approvalRun, 'waiting')
                return
              }
              case 'todo_update': {
                const parsed = todoUpdatePayloadSchema.safeParse(event.payload)
                if (!parsed.success || !parsed.data.agent_id) return
                upsertStreamTodos(
                  storeId,
                  streamId,
                  parsed.data.agent_id,
                  parsed.data.todos,
                  agentDisplayName(parsed.data.agent_id, parsed.data.display_name),
                )
                return
              }
              case 'warning': {
                const message = messageFromPayload(event.payload, 'Stream warning')
                pushWarning(storeId, message)
                appendStreamNotice(storeId, streamId, {
                  type: 'warning',
                  message,
                })
                return
              }
              case 'conversation_updated': {
                const parsed = parseConversationUpdatedEvent(event)
                onConversationUpdated?.(parsed.payload)
                return
              }
              case 'error': {
                const message = messageFromPayload(event.payload, 'Stream failed')
                setError(message)
                erroredStreamIdsRef.current.add(streamId)
                sendFailuresRef.current.set(id, message)
                markStreamRunError(storeId, streamId, message)
                clearStreamInFlight(storeId, streamId)
                clearActiveAgent(storeId, undefined, streamId)
                pushWarning(storeId, message)
                return
              }
              case 'done': {
                if (!erroredStreamIdsRef.current.has(streamId)) {
                  markStreamRunDone(storeId, streamId)
                }
                finishStream(id, streamId)
                window.setTimeout(() => clearToolActivity(storeId), 4_000)
                return
              }
              default: {
                const _exhaustive: never = event.kind
                return _exhaustive
              }
            }
          },
          onRetry: (attempt, delayMs) => {
            retriesRef.current.set(id, { attempt, delayMs })
            refreshRetry()
          },
          onError: (err) => {
            const message = err instanceof Error ? err.message : String(err)
            clearRetry(id)
            setRetryExhausted(
              err instanceof Error && err.name === 'SseRetryExhaustedError',
            )
            rejectSendBeforeAcknowledgement(id, err)
            setError(message)
            sendFailuresRef.current.set(id, message)
            const streamId = streamIdsRef.current.get(id)
            if (streamId) {
              erroredStreamIdsRef.current.add(streamId)
              markStreamRunError(storeId, streamId, message)
              clearStreamInFlight(storeId, streamId)
            } else {
              clearInFlight(storeId)
            }
            finishStream(id, streamId)
          },
          onClose: () => {
            finishStream(id)
          },
          },
        })
        streamsRef.current.set(id, ctrl)
        startSend(storeId, cancelOwned)
        refreshActiveCount()
      } catch (error) {
        const startupError = asError(error, 'Unable to start the message stream')
        setError(startupError.message)
        schedulerTurnByRequestRef.current.delete(id)
        // The run never reached the server, so there is nothing to announce —
        // the composer already shows why the send did not leave.
        finishActivityRun(id, 'cancelled')
        rejectSendBeforeAcknowledgement(id, startupError)
      }
      return acknowledgement.promise
    },
    [
      acknowledgeSend,
      addStreamAgentStart,
      acceptsStreamEvent,
      applySchedulerEvent,
      appendMessage,
      appendStreamNotice,
      clearActiveAgent,
      clearAgentInFlight,
      clearInFlight,
      clearRetry,
      clearStreamInFlight,
      clearToolActivity,
      clearWarnings,
      cancelOwned,
      completePendingCancellation,
      currentUserId,
      clearStreamingStreamDraft,
      finalizeInFlight,
      finalizeStreamDraft,
      finishActivityRun,
      finishStream,
      groupId,
      invalidateTurn,
      markActivityRunWaiting,
      markStreamRunDone,
      markStreamRunError,
      markStreamRunWaitingForUser,
      patchInFlight,
      patchStreamDraft,
      patchStreamReasoning,
      pushToolActivity,
      pushWarning,
      qc,
      refreshActiveCount,
      refreshRetry,
      rejectSendBeforeAcknowledgement,
      scope,
      setActiveAgent,
      setStreamAgentContextUsage,
      settleOptimisticUserMessage,
      stateKey,
      startActivityRun,
      startOptimisticUserMessage,
      startSend,
      startStreamRun,
      threadId,
      token,
      onConversationUpdated,
      upsertStreamExternalRun,
      upsertStreamTodos,
      upsertStreamTool,
    ],
  )

  return {
    send,
    cancel: activeSend?.cancel ?? cancelOwned,
    isStreaming: Boolean(activeSend),
    activeStreamCount: activeStreamCount || (activeSend ? 1 : 0),
    error,
    retry,
    retryExhausted,
  }
}
