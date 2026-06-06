/**
 * Stream-send messages to a group.
 *
 * Multiple sends may be active at once. Each SSE stream carries a backend
 * `stream_id` (the triggering user message id), which keeps in-flight agent
 * bubbles separate even when the same agent is replying to more than one user
 * message.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import type { HumanInputRequest } from '@/lib/humanInput'
import { openSseStream } from '@/lib/sse'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ActiveAgent, ToolActivity, ToolActivityStatus } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface StreamPayload {
  stream_id?: string | null
}

interface TokenPayload extends StreamPayload {
  agent_id: string
  delta: string
}

interface AgentIdentityPayload extends StreamPayload {
  agent_id: string
  display_name?: string
  round?: number
}

interface AgentErrorPayload extends StreamPayload {
  agent_id: string
  display_name: string
  error: string
  round?: number
}

interface WaitingForUserPayload extends StreamPayload {
  message?: string
  agent_id?: string
  display_name?: string
  input_request?: HumanInputRequest
  round?: number
}

type DonePayload = StreamPayload

interface ToolCallPayload extends StreamPayload {
  agent_id?: string
  display_name?: string
  tool_call_id?: string
  tool_name?: string
  status?: ToolActivityStatus
  args_summary?: string
  result_summary?: string
  input_request?: HumanInputRequest
  round?: number
}

interface ExternalAgentRunPayload extends StreamPayload {
  run_id?: string
  agent_id?: string
  display_name?: string
  adapter?: string
  status?: string
  cwd?: string
  exit_code?: number
  summary?: string
  round?: number
}

const TOOL_ACTIVITY_STATUSES = new Set<ToolActivityStatus>([
  'started',
  'completed',
  'failed',
  'unavailable',
  'setup_required',
  'workspace_required',
  'input_required',
  'approval_required',
])

function safeJson<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    return null
  }
}

function normalizeToolStatus(status: unknown): ToolActivityStatus {
  return typeof status === 'string' && TOOL_ACTIVITY_STATUSES.has(status as ToolActivityStatus)
    ? (status as ToolActivityStatus)
    : 'unavailable'
}

function externalRunStatus(status: string | undefined): ToolActivityStatus {
  if (status === 'running') return 'started'
  if (status === 'completed') return 'completed'
  return 'failed'
}

function requestId(): string {
  return crypto.randomUUID()
}

export function useSendMessageStream(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
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
  const patchStreamDraft = useMessageStore((s) => s.patchStreamDraft)
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
  const agentNamesRef = useRef<Map<string, string>>(new Map())

  const refreshActiveCount = useCallback(() => {
    setActiveStreamCount(streamsRef.current.size)
  }, [])

  useEffect(() => {
    const streams = streamsRef.current
    const streamIds = streamIdsRef.current
    const agentNames = agentNamesRef.current
    return () => {
      for (const ctrl of streams.values()) {
        ctrl.abort()
      }
      streams.clear()
      streamIds.clear()
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

      const ctrl = openSseStream({
        url: `/api/v1/groups/${groupId}/messages/stream`,
        body: { content },
        token,
        handlers: {
          onEvent: (event, data) => {
            const streamIdFor = (payload?: StreamPayload | null) =>
              payload?.stream_id ?? streamIdsRef.current.get(id) ?? null
            const agentDisplayName = (
              streamId: string | null,
              agentId: string | undefined,
              fallback?: string,
            ) => {
              if (fallback) return fallback
              if (streamId && agentId) {
                return agentNamesRef.current.get(`${streamId}:${agentId}`) ?? 'Agent'
              }
              return 'Agent'
            }

            if (event === 'user_message') {
              const msg = safeJson<Message>(data)
              if (msg) {
                streamIdsRef.current.set(id, msg.id)
                appendMessage(groupId, msg)
                startStreamRun(groupId, msg)
              }
              return
            }
            if (event === 'agent_start') {
              const info = safeJson<ActiveAgent>(data)
              if (info) {
                const streamId = streamIdFor(info)
                if (streamId) {
                  streamIdsRef.current.set(id, streamId)
                  agentNamesRef.current.set(`${streamId}:${info.agent_id}`, info.display_name)
                  addStreamAgentStart(groupId, streamId, { ...info, stream_id: streamId })
                }
                setActiveAgent(groupId, info)
              }
              return
            }
            if (event === 'token') {
              const payload = safeJson<TokenPayload>(data)
              if (payload?.agent_id && payload.delta) {
                const streamId = streamIdFor(payload)
                if (streamId) {
                  streamIdsRef.current.set(id, streamId)
                  patchStreamDraft(
                    groupId,
                    streamId,
                    payload.agent_id,
                    payload.delta,
                    agentDisplayName(streamId, payload.agent_id),
                  )
                }
                patchInFlight(groupId, payload.agent_id, payload.delta, streamId)
              }
              return
            }
            if (event === 'agent_message') {
              const msg = safeJson<Message>(data)
              if (msg) {
                const streamId = msg.reply_to_message_id ?? streamIdsRef.current.get(id) ?? null
                if (streamId) {
                  streamIdsRef.current.set(id, streamId)
                  finalizeStreamDraft(
                    groupId,
                    streamId,
                    msg,
                    agentDisplayName(streamId, msg.sender_id ?? undefined),
                  )
                }
                finalizeInFlight(groupId, msg)
              }
              void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
              return
            }
            if (event === 'agent_silent') {
              const info = safeJson<AgentIdentityPayload>(data)
              if (info?.agent_id) {
                const streamId = streamIdFor(info)
                if (streamId) {
                  streamIdsRef.current.set(id, streamId)
                  appendStreamNotice(groupId, streamId, {
                    type: 'agent_silent',
                    agent_id: info.agent_id,
                    display_name: agentDisplayName(streamId, info.agent_id, info.display_name),
                    message: 'No visible reply',
                  })
                }
                clearAgentInFlight(groupId, info.agent_id, streamId)
                clearActiveAgent(groupId, info.agent_id, streamId)
              }
              return
            }
            if (event === 'agent_handoff') {
              const payload = safeJson<AgentIdentityPayload>(data)
              const streamId = streamIdFor(payload)
              if (streamId) {
                streamIdsRef.current.set(id, streamId)
                appendStreamNotice(groupId, streamId, {
                  type: 'agent_handoff',
                  agent_id: payload?.agent_id,
                  display_name: agentDisplayName(streamId, payload?.agent_id, payload?.display_name),
                  message: 'Delegated to another agent',
                })
              }
              return
            }
            if (event === 'tool_call_start' || event === 'tool_call_result') {
              const payload = safeJson<ToolCallPayload>(data)
              if (payload?.tool_call_id) {
                const streamId = streamIdFor(payload)
                if (streamId) streamIdsRef.current.set(id, streamId)
                const status = normalizeToolStatus(payload.status)
                const activity: ToolActivity = {
                  id: payload.tool_call_id,
                  agent_id: payload.agent_id ?? 'unknown-agent',
                  display_name: agentDisplayName(
                    streamId,
                    payload.agent_id,
                    payload.display_name,
                  ),
                  tool_name: payload.tool_name ?? 'Unknown tool',
                  status,
                  args_summary: payload.args_summary,
                  result_summary: payload.result_summary,
                  input_request: payload.input_request,
                }
                pushToolActivity(groupId, activity)
                if (streamId) upsertStreamTool(groupId, streamId, activity)
                if (event === 'tool_call_result') {
                  void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
                }
              }
              return
            }
            if (event === 'external_agent_run') {
              const payload = safeJson<ExternalAgentRunPayload>(data)
              if (payload?.run_id) {
                const streamId = streamIdFor(payload)
                if (streamId) streamIdsRef.current.set(id, streamId)
                const displayName = agentDisplayName(
                  streamId,
                  payload.agent_id,
                  payload.display_name,
                )
                pushToolActivity(groupId, {
                  id: payload.run_id,
                  agent_id: payload.agent_id ?? 'unknown-agent',
                  display_name: displayName,
                  tool_name: `External CLI: ${payload.adapter ?? 'unknown'}`,
                  status: externalRunStatus(payload.status),
                  args_summary: payload.cwd,
                  result_summary: payload.summary,
                })
                if (streamId) {
                  upsertStreamExternalRun(groupId, streamId, {
                    run_id: payload.run_id,
                    agent_id: payload.agent_id ?? 'unknown-agent',
                    display_name: displayName,
                    adapter: payload.adapter,
                    status: payload.status,
                    cwd: payload.cwd,
                    exit_code: payload.exit_code,
                    summary: payload.summary,
                  })
                }
                if (payload.status && payload.status !== 'running') {
                  void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
                }
              }
              return
            }
            if (event === 'silence') {
              pushWarning(groupId, 'No one replied')
              const streamId = streamIdsRef.current.get(id)
              if (streamId) {
                appendStreamNotice(groupId, streamId, {
                  type: 'warning',
                  message: 'No one replied',
                })
              }
              return
            }
            if (event === 'waiting_for_user') {
              const payload = safeJson<WaitingForUserPayload>(data)
              const streamId = streamIdFor(payload)
              if (streamId) {
                streamIdsRef.current.set(id, streamId)
                appendStreamNotice(groupId, streamId, {
                  type: 'waiting_for_user',
                  agent_id: payload?.agent_id,
                  display_name: agentDisplayName(
                    streamId,
                    payload?.agent_id,
                    payload?.display_name,
                  ),
                  message: payload?.message || 'Waiting for your input',
                  input_request: payload?.input_request,
                })
              }
              pushWarning(groupId, payload?.message || 'Waiting for your input')
              return
            }
            if (event === 'agent_error') {
              const err = safeJson<AgentErrorPayload>(data)
              if (err) {
                const streamId = streamIdFor(err)
                if (streamId) {
                  streamIdsRef.current.set(id, streamId)
                  appendStreamNotice(groupId, streamId, {
                    type: 'agent_error',
                    agent_id: err.agent_id,
                    display_name: agentDisplayName(streamId, err.agent_id, err.display_name),
                    message: err.error,
                  })
                }
                clearAgentInFlight(groupId, err.agent_id, streamId)
                clearActiveAgent(groupId, err.agent_id, streamId)
                pushWarning(groupId, `Agent "${err.display_name}" failed: ${err.error}`)
              }
              return
            }
            if (event === 'warning') {
              pushWarning(groupId, data)
              const streamId = streamIdsRef.current.get(id)
              if (streamId) {
                appendStreamNotice(groupId, streamId, {
                  type: 'warning',
                  message: data,
                })
              }
              return
            }
            if (event === 'done') {
              const payload = safeJson<DonePayload>(data)
              const streamId = streamIdFor(payload)
              if (streamId) markStreamRunDone(groupId, streamId)
              finishStream(id, streamId)
              window.setTimeout(() => clearToolActivity(groupId), 4_000)
            }
          },
          onError: (err) => {
            const message = err instanceof Error ? err.message : String(err)
            setError(message)
            const streamId = streamIdsRef.current.get(id)
            if (streamId) markStreamRunError(groupId, streamId, message)
            if (streamId) {
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
      finalizeStreamDraft,
      finalizeInFlight,
      finishStream,
      groupId,
      markStreamRunDone,
      markStreamRunError,
      patchInFlight,
      patchStreamDraft,
      pushToolActivity,
      pushWarning,
      qc,
      refreshActiveCount,
      setActiveAgent,
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
