/**
 * Stream-send messages to a group through the API v2 typed SSE contract.
 *
 * Multiple sends may be active at once. API v2 uses a runtime `stream_id`
 * for each SSE run, while the persisted user message has its own id in the
 * `user_message` payload. Keep those ids separate so concurrent agent drafts
 * never share transient state.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { z } from 'zod'

import { fetchJson } from '@/lib/api-v2/client'
import {
  parseConversationUpdatedEvent,
  parseGroupTurnTrace,
  parseSchedulerStreamEvent,
} from '@/lib/api-v2/schemas'
import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type {
  ConversationUpdatedPayload,
  SchedulerStreamUpdate,
  StreamEvent,
} from '@/lib/api-v2/types'
import {
  conversationApiPath,
  conversationMessagesKey,
  type ConversationScope,
} from '@/hooks/useGroupMessages'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ActiveAgent, ToolActivity, ToolActivityStatus } from '@/stores/messageStore'
import type { ContextUsage, Message } from '@/types/api'

const userMessagePayloadSchema = z.object({
  message_id: z.string(),
  thread_id: z.string().nullable().optional(),
  content: z.string().nullable().optional(),
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

const waitingForUserPayloadSchema = z.object({
  agent_id: z.string().optional(),
  display_name: z.string().optional(),
  message: z.string().optional(),
})

const toolCallPayloadSchema = z.object({
  agent_id: z.string().optional(),
  display_name: z.string().optional(),
  tool_call_id: z.string().optional(),
  tool_name: z.string().optional(),
  status: z.string().optional(),
  args_summary: z.string().optional(),
  result_summary: z.string().optional(),
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

type StreamProtocol = 'scheduler' | 'legacy'

interface PendingCancellation {
  requestIds: Set<string>
  completing: boolean
  promise: Promise<void>
  resolve: () => void
}

interface SendMessageStreamOptions {
  scope?: ConversationScope
  onConversationUpdated?: (payload: ConversationUpdatedPayload) => void
}

export function useSendMessageStream(
  groupId: string | undefined,
  schedulerEnabled: boolean,
  options: SendMessageStreamOptions = {},
) {
  const scope = options.scope ?? 'groups'
  const onConversationUpdated = options.onConversationUpdated
  const token = useAuthStore((s) => s.token)
  const currentUserId = useAuthStore((s) => s.user?.id ?? null)
  const appendMessage = useMessageStore((s) => s.appendMessage)
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
  const upsertStreamExternalRun = useMessageStore((s) => s.upsertStreamExternalRun)
  const appendStreamNotice = useMessageStore((s) => s.appendStreamNotice)
  const applySchedulerEvent = useMessageStore((s) => s.applySchedulerEvent)
  const acceptsStreamEvent = useMessageStore((s) => s.acceptsStreamEvent)
  const markStreamRunWaitingForUser = useMessageStore((s) => s.markStreamRunWaitingForUser)
  const reconcileSchedulerTurn = useMessageStore((s) => s.reconcileSchedulerTurn)
  const detachStreamRun = useMessageStore((s) => s.detachStreamRun)
  const markStreamRunDone = useMessageStore((s) => s.markStreamRunDone)
  const markStreamRunError = useMessageStore((s) => s.markStreamRunError)
  const markStreamRunCancelled = useMessageStore((s) => s.markStreamRunCancelled)
  const qc = useQueryClient()

  const [activeStreamCount, setActiveStreamCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const streamsRef = useRef<Map<string, AbortController>>(new Map())
  const streamIdsRef = useRef<Map<string, string>>(new Map())
  const erroredStreamIdsRef = useRef<Set<string>>(new Set())
  const agentNamesRef = useRef<Map<string, string>>(new Map())
  const streamProtocolByRequestRef = useRef<Map<string, StreamProtocol>>(new Map())
  const schedulerTurnByRequestRef = useRef<Map<string, string>>(new Map())
  const pendingCancellationRef = useRef<PendingCancellation | null>(null)

  const refreshActiveCount = useCallback(() => {
    setActiveStreamCount(streamsRef.current.size)
  }, [])

  useEffect(() => {
    const abandonedGroupId = groupId
    const streams = streamsRef.current
    const streamIds = streamIdsRef.current
    const erroredStreamIds = erroredStreamIdsRef.current
    const agentNames = agentNamesRef.current
    const streamProtocols = streamProtocolByRequestRef.current
    const schedulerTurns = schedulerTurnByRequestRef.current
    return () => {
      const hadAbandonedStreams = streams.size > 0
      const pendingCancellation = pendingCancellationRef.current
      if (pendingCancellation) {
        pendingCancellation.resolve()
        pendingCancellationRef.current = null
      }
      for (const [requestId, ctrl] of streams) {
        ctrl.abort()
        const streamId = streamIds.get(requestId)
        if (abandonedGroupId && streamId) {
          detachStreamRun(abandonedGroupId, streamId)
        }
      }
      if (abandonedGroupId && hadAbandonedStreams) {
        clearToolActivity(abandonedGroupId)
      }
      streams.clear()
      streamIds.clear()
      erroredStreamIds.clear()
      agentNames.clear()
      streamProtocols.clear()
      schedulerTurns.clear()
      setActiveStreamCount(0)
    }
  }, [clearToolActivity, detachStreamRun, groupId])

  const invalidate = useCallback(() => {
    if (!groupId) return
    void qc.invalidateQueries({ queryKey: conversationMessagesKey(scope, groupId) })
    void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
  }, [groupId, qc, scope])

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
      const protocol = streamProtocolByRequestRef.current.get(id)
      if (!protocol) return
      if (protocol === 'scheduler' && !schedulerTurnByRequestRef.current.has(id)) return
    }

    pending.completing = true
    const schedulerRequestIds = activeRequestIds.filter(
      (id) => streamProtocolByRequestRef.current.get(id) === 'scheduler',
    )
    const turnIds = new Set(
      schedulerRequestIds
        .map((id) => schedulerTurnByRequestRef.current.get(id))
        .filter((turnId): turnId is string => Boolean(turnId)),
    )

    if (turnIds.size > 0 && (!groupId || !token)) {
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
          reconcileSchedulerTurn(groupId, trace)
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
    const legacyStreamIds = activeRequestIds
      .filter((id) => streamProtocolByRequestRef.current.get(id) === 'legacy')
      .map((id) => streamIdsRef.current.get(id))
      .filter((streamId): streamId is string => Boolean(streamId))
    for (const id of activeRequestIds) {
      streamsRef.current.get(id)?.abort()
      streamsRef.current.delete(id)
      streamIdsRef.current.delete(id)
      streamProtocolByRequestRef.current.delete(id)
      schedulerTurnByRequestRef.current.delete(id)
    }
    erroredStreamIdsRef.current.clear()
    agentNamesRef.current.clear()
    refreshActiveCount()

    if (groupId) {
      if (legacyStreamIds.length > 0) {
        markStreamRunCancelled(groupId, legacyStreamIds)
      }
      if (streamIds.length > 0) {
        for (const streamId of streamIds) {
          clearStreamInFlight(groupId, streamId)
          clearActiveAgent(groupId, undefined, streamId)
        }
      } else {
        clearInFlight(groupId)
      }
    }
    pendingCancellationRef.current = null
    setError(null)
    pending.resolve()
    window.setTimeout(invalidate, 700)
  }, [
    clearActiveAgent,
    clearInFlight,
    clearStreamInFlight,
    groupId,
    invalidate,
    markStreamRunCancelled,
    reconcileSchedulerTurn,
    refreshActiveCount,
    token,
  ])

  const finishStream = useCallback(
    (id: string, streamId?: string | null) => {
      streamsRef.current.delete(id)
      const resolvedStreamId = streamId ?? streamIdsRef.current.get(id)
      if (resolvedStreamId && groupId) {
        clearActiveAgent(groupId, undefined, resolvedStreamId)
      }
      streamIdsRef.current.delete(id)
      streamProtocolByRequestRef.current.delete(id)
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
      if (groupId && streamsRef.current.size === 0) {
        clearActiveAgent(groupId)
      }
      invalidate()
      void completePendingCancellation()
    },
    [
      clearActiveAgent,
      completePendingCancellation,
      groupId,
      invalidate,
      refreshActiveCount,
    ],
  )

  const send = useCallback(
    (content: string) => {
      if (!groupId || !token) return
      if (pendingCancellationRef.current) {
        setError('Cancellation is in progress')
        return
      }
      const id = requestId()
      streamProtocolByRequestRef.current.set(
        id,
        schedulerEnabled ? 'scheduler' : 'legacy',
      )
      setError(null)
      clearWarnings(groupId)
      clearToolActivity(groupId)

      const ctrl = openApiV2SseStream({
        url: `/api/v2${conversationApiPath(scope, groupId)}/messages/stream`,
        body: { content },
        token,
        handlers: {
          onEvent: (event) => {
            const streamId = event.stream_id
            streamIdsRef.current.set(id, streamId)
            const schedulerUpdate = parseSchedulerStreamEvent(event)
            if (schedulerUpdate) {
              streamProtocolByRequestRef.current.set(id, 'scheduler')
              schedulerTurnByRequestRef.current.set(id, schedulerUpdate.payload.turn_id)
              void completePendingCancellation()
              if (!applySchedulerEvent(groupId, streamId, schedulerUpdate)) return
              if (isTerminalSchedulerUpdate(schedulerUpdate)) {
                invalidateTurn(schedulerUpdate.payload.turn_id)
              }
              if (schedulerUpdate.kind === 'done') {
                if (!erroredStreamIdsRef.current.has(streamId)) {
                  markStreamRunDone(groupId, streamId)
                }
                finishStream(id, streamId)
                window.setTimeout(() => clearToolActivity(groupId), 4_000)
              }
              return
            }
            if (!acceptsStreamEvent(groupId, streamId)) return
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
                appendMessage(groupId, msg)
                startStreamRun(groupId, streamId, msg)
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
                addStreamAgentStart(groupId, streamId, info)
                setActiveAgent(groupId, info)
                return
              }
              case 'context_usage': {
                const parsed = contextUsagePayloadSchema.safeParse(event.payload)
                if (!parsed.success || !parsed.data.agent_id) return
                const usage = contextUsageFromPayload(parsed.data)
                if (usage) {
                  setStreamAgentContextUsage(groupId, streamId, parsed.data.agent_id, usage)
                }
                return
              }
              case 'token': {
                const parsed = tokenPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const delta = parsed.data.delta ?? parsed.data.text ?? ''
                if (!delta) return
                patchStreamDraft(
                  groupId,
                  streamId,
                  parsed.data.agent_id,
                  delta,
                  agentDisplayName(parsed.data.agent_id),
                )
                patchInFlight(groupId, parsed.data.agent_id, delta, streamId)
                return
              }
              case 'reasoning': {
                const parsed = tokenPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                const delta = parsed.data.delta ?? parsed.data.text ?? ''
                if (!delta) return
                patchStreamReasoning(
                  groupId,
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
                const turnId = useMessageStore.getState().streamRunsByGroup[groupId]?.[streamId]
                  ?.turn_id ?? null
                const msg = buildAgentMessage(groupId, event, parsed.data, turnId)
                finalizeStreamDraft(
                  groupId,
                  streamId,
                  msg,
                  agentDisplayName(parsed.data.agent_id, parsed.data.display_name),
                )
                finalizeInFlight(groupId, msg)
                void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
                void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
                return
              }
              case 'agent_silent': {
                const parsed = agentStartPayloadSchema.safeParse(event.payload)
                if (!parsed.success) return
                clearStreamingStreamDraft(groupId, streamId, parsed.data.agent_id)
                appendStreamNotice(groupId, streamId, {
                  type: 'agent_silent',
                  agent_id: parsed.data.agent_id,
                  display_name: agentDisplayName(
                    parsed.data.agent_id,
                    parsed.data.display_name,
                  ),
                  message: 'No visible reply',
                })
                clearAgentInFlight(groupId, parsed.data.agent_id, streamId)
                clearActiveAgent(groupId, parsed.data.agent_id, streamId)
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
                }
                pushToolActivity(groupId, activity)
                upsertStreamTool(groupId, streamId, activity)
                if (event.kind === 'tool_call_result') {
                  void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
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
                pushToolActivity(groupId, {
                  id: parsed.data.run_id,
                  agent_id: parsed.data.agent_id,
                  display_name: displayName,
                  tool_name: `External CLI: ${parsed.data.adapter ?? 'unknown'}`,
                  status: externalRunStatus(parsed.data.status),
                  args_summary: parsed.data.cwd,
                  result_summary: parsed.data.summary,
                })
                upsertStreamExternalRun(groupId, streamId, {
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
                  void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
                }
                return
              }
              case 'silence': {
                pushWarning(groupId, 'No one replied')
                appendStreamNotice(groupId, streamId, {
                  type: 'warning',
                  message: 'No one replied',
                })
                return
              }
              case 'waiting_for_user': {
                const parsed = waitingForUserPayloadSchema.safeParse(event.payload)
                const payload = parsed.success ? parsed.data : undefined
                const message = payload?.message ?? 'Waiting for your input'
                appendStreamNotice(groupId, streamId, {
                  type: 'waiting_for_user',
                  agent_id: payload?.agent_id,
                  display_name: agentDisplayName(payload?.agent_id, payload?.display_name),
                  message,
                })
                pushWarning(groupId, message)
                const turnId = markStreamRunWaitingForUser(groupId, streamId)
                if (turnId) invalidateTurn(turnId)
                return
              }
              case 'warning': {
                const message = messageFromPayload(event.payload, 'Stream warning')
                pushWarning(groupId, message)
                appendStreamNotice(groupId, streamId, {
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
                markStreamRunError(groupId, streamId, message)
                clearStreamInFlight(groupId, streamId)
                clearActiveAgent(groupId, undefined, streamId)
                pushWarning(groupId, message)
                return
              }
              case 'done': {
                if (!erroredStreamIdsRef.current.has(streamId)) {
                  markStreamRunDone(groupId, streamId)
                }
                finishStream(id, streamId)
                window.setTimeout(() => clearToolActivity(groupId), 4_000)
                return
              }
              default: {
                const _exhaustive: never = event.kind
                return _exhaustive
              }
            }
          },
          onError: (err) => {
            const message = err instanceof Error ? err.message : String(err)
            setError(message)
            const streamId = streamIdsRef.current.get(id)
            if (streamId) {
              erroredStreamIdsRef.current.add(streamId)
              markStreamRunError(groupId, streamId, message)
              clearStreamInFlight(groupId, streamId)
            } else {
              clearInFlight(groupId)
            }
            finishStream(id, streamId)
          },
          onClose: () => {
            finishStream(id)
          },
        },
      })
      streamsRef.current.set(id, ctrl)
      refreshActiveCount()
    },
    [
      addStreamAgentStart,
      acceptsStreamEvent,
      applySchedulerEvent,
      appendMessage,
      appendStreamNotice,
      clearActiveAgent,
      clearAgentInFlight,
      clearInFlight,
      clearStreamInFlight,
      clearToolActivity,
      clearWarnings,
      completePendingCancellation,
      currentUserId,
      clearStreamingStreamDraft,
      finalizeInFlight,
      finalizeStreamDraft,
      finishStream,
      groupId,
      invalidateTurn,
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
    schedulerEnabled,
    scope,
      setActiveAgent,
      setStreamAgentContextUsage,
      startStreamRun,
    token,
    onConversationUpdated,
      upsertStreamExternalRun,
      upsertStreamTool,
    ],
  )

  const cancel = useCallback((): Promise<void> => {
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

  return { send, cancel, isStreaming: activeStreamCount > 0, activeStreamCount, error }
}
