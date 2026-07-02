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

import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type { StreamEvent } from '@/lib/api-v2/types'
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
    reply_to_message_id: null,
    created_at: nowIso(),
  }
}

function buildAgentMessage(
  groupId: string,
  event: StreamEvent,
  payload: AgentMessagePayload,
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
    reply_to_message_id: event.stream_id,
    created_at: nowIso(),
  }
}

export function useSendMessageStream(groupId: string | undefined) {
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

  const refreshActiveCount = useCallback(() => {
    setActiveStreamCount(streamsRef.current.size)
  }, [])

  useEffect(() => {
    const streams = streamsRef.current
    const streamIds = streamIdsRef.current
    const erroredStreamIds = erroredStreamIdsRef.current
    const agentNames = agentNamesRef.current
    return () => {
      for (const ctrl of streams.values()) {
        ctrl.abort()
      }
      streams.clear()
      streamIds.clear()
      erroredStreamIds.clear()
      agentNames.clear()
    }
  }, [])

  const invalidate = useCallback(() => {
    if (!groupId) return
    void qc.invalidateQueries({ queryKey: ['groups', groupId, 'messages'] })
    void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
  }, [groupId, qc])

  const finishStream = useCallback(
    (id: string, streamId?: string | null) => {
      streamsRef.current.delete(id)
      const resolvedStreamId = streamId ?? streamIdsRef.current.get(id)
      if (resolvedStreamId && groupId) {
        clearActiveAgent(groupId, undefined, resolvedStreamId)
      }
      streamIdsRef.current.delete(id)
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
    },
    [clearActiveAgent, groupId, invalidate, refreshActiveCount],
  )

  const send = useCallback(
    (content: string) => {
      if (!groupId || !token) return
      const id = requestId()
      setError(null)
      clearWarnings(groupId)
      clearToolActivity(groupId)

      const ctrl = openApiV2SseStream({
        url: `/api/v2/groups/${groupId}/messages/stream`,
        body: { content },
        token,
        handlers: {
          onEvent: (event) => {
            const streamId = event.stream_id
            streamIdsRef.current.set(id, streamId)
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
                const msg = buildAgentMessage(groupId, event, parsed.data)
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
      appendMessage,
      appendStreamNotice,
      clearActiveAgent,
      clearAgentInFlight,
      clearInFlight,
      clearStreamInFlight,
      clearToolActivity,
      clearWarnings,
      currentUserId,
      clearStreamingStreamDraft,
      finalizeInFlight,
      finalizeStreamDraft,
      finishStream,
      groupId,
      markStreamRunDone,
      markStreamRunError,
      patchInFlight,
      patchStreamDraft,
      patchStreamReasoning,
      pushToolActivity,
      pushWarning,
      qc,
      refreshActiveCount,
      setActiveAgent,
      setStreamAgentContextUsage,
      startStreamRun,
      token,
      upsertStreamExternalRun,
      upsertStreamTool,
    ],
  )

  const cancel = useCallback(() => {
    const streamIds = Array.from(new Set(streamIdsRef.current.values()))
    for (const ctrl of streamsRef.current.values()) {
      ctrl.abort()
    }
    streamsRef.current.clear()
    streamIdsRef.current.clear()
    erroredStreamIdsRef.current.clear()
    agentNamesRef.current.clear()
    setActiveStreamCount(0)
    if (groupId) {
      markStreamRunCancelled(groupId, streamIds)
      if (streamIds.length > 0) {
        for (const streamId of streamIds) {
          clearStreamInFlight(groupId, streamId)
        }
      } else {
        clearInFlight(groupId)
      }
    }
    window.setTimeout(invalidate, 700)
  }, [clearInFlight, clearStreamInFlight, groupId, invalidate, markStreamRunCancelled])

  return { send, cancel, isStreaming: activeStreamCount > 0, activeStreamCount, error }
}
