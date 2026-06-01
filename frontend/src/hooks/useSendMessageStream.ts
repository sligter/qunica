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
}

interface AgentErrorPayload extends StreamPayload {
  agent_id: string
  display_name: string
  error: string
}

interface WaitingForUserPayload extends StreamPayload {
  message?: string
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
  const qc = useQueryClient()

  const [activeStreamCount, setActiveStreamCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const streamsRef = useRef<Map<string, AbortController>>(new Map())
  const streamIdsRef = useRef<Map<string, string>>(new Map())

  const refreshActiveCount = useCallback(() => {
    setActiveStreamCount(streamsRef.current.size)
  }, [])

  useEffect(() => {
    const streams = streamsRef.current
    const streamIds = streamIdsRef.current
    return () => {
      for (const ctrl of streams.values()) {
        ctrl.abort()
      }
      streams.clear()
      streamIds.clear()
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

      const ctrl = openSseStream({
        url: `/api/v1/groups/${groupId}/messages/stream`,
        body: { content },
        token,
        handlers: {
          onEvent: (event, data) => {
            if (event === 'user_message') {
              const msg = safeJson<Message>(data)
              if (msg) {
                streamIdsRef.current.set(id, msg.id)
                appendMessage(groupId, msg)
              }
              return
            }
            if (event === 'agent_start') {
              const info = safeJson<ActiveAgent>(data)
              if (info) {
                if (info.stream_id) streamIdsRef.current.set(id, info.stream_id)
                setActiveAgent(groupId, info)
              }
              return
            }
            if (event === 'token') {
              const payload = safeJson<TokenPayload>(data)
              if (payload?.agent_id && payload.delta) {
                if (payload.stream_id) streamIdsRef.current.set(id, payload.stream_id)
                patchInFlight(groupId, payload.agent_id, payload.delta, payload.stream_id ?? null)
              }
              return
            }
            if (event === 'agent_message') {
              const msg = safeJson<Message>(data)
              if (msg) {
                finalizeInFlight(groupId, msg)
              }
              void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
              return
            }
            if (event === 'agent_silent') {
              const info = safeJson<AgentIdentityPayload>(data)
              if (info?.agent_id) {
                if (info.stream_id) streamIdsRef.current.set(id, info.stream_id)
                clearAgentInFlight(groupId, info.agent_id, info.stream_id ?? null)
                clearActiveAgent(groupId, info.agent_id, info.stream_id ?? null)
              }
              return
            }
            if (event === 'tool_call_start' || event === 'tool_call_result') {
              const payload = safeJson<ToolCallPayload>(data)
              if (payload?.tool_call_id) {
                if (payload.stream_id) streamIdsRef.current.set(id, payload.stream_id)
                const status = normalizeToolStatus(payload.status)
                const activity: ToolActivity = {
                  id: payload.tool_call_id,
                  agent_id: payload.agent_id ?? 'unknown-agent',
                  display_name: payload.display_name ?? 'Agent',
                  tool_name: payload.tool_name ?? 'Unknown tool',
                  status,
                  args_summary: payload.args_summary,
                  result_summary: payload.result_summary,
                }
                pushToolActivity(groupId, activity)
                if (event === 'tool_call_result') {
                  void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
                }
              }
              return
            }
            if (event === 'external_agent_run') {
              const payload = safeJson<ExternalAgentRunPayload>(data)
              if (payload?.run_id) {
                if (payload.stream_id) streamIdsRef.current.set(id, payload.stream_id)
                pushToolActivity(groupId, {
                  id: payload.run_id,
                  agent_id: payload.agent_id ?? 'unknown-agent',
                  display_name: payload.display_name ?? 'Agent',
                  tool_name: `External CLI: ${payload.adapter ?? 'unknown'}`,
                  status: externalRunStatus(payload.status),
                  args_summary: payload.cwd,
                  result_summary: payload.summary,
                })
                if (payload.status && payload.status !== 'running') {
                  void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
                }
              }
              return
            }
            if (event === 'silence') {
              pushWarning(groupId, 'No one replied')
              return
            }
            if (event === 'waiting_for_user') {
              const payload = safeJson<WaitingForUserPayload>(data)
              if (payload?.stream_id) streamIdsRef.current.set(id, payload.stream_id)
              pushWarning(groupId, payload?.message || 'Waiting for your input')
              return
            }
            if (event === 'agent_error') {
              const err = safeJson<AgentErrorPayload>(data)
              if (err) {
                if (err.stream_id) streamIdsRef.current.set(id, err.stream_id)
                clearAgentInFlight(groupId, err.agent_id, err.stream_id ?? null)
                clearActiveAgent(groupId, err.agent_id, err.stream_id ?? null)
                pushWarning(groupId, `Agent "${err.display_name}" failed: ${err.error}`)
              }
              return
            }
            if (event === 'warning') {
              pushWarning(groupId, data)
              return
            }
            if (event === 'done') {
              const payload = safeJson<DonePayload>(data)
              finishStream(id, payload?.stream_id)
              window.setTimeout(() => clearToolActivity(groupId), 4_000)
            }
          },
          onError: (err) => {
            setError(err instanceof Error ? err.message : String(err))
            const streamId = streamIdsRef.current.get(id)
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
      appendMessage,
      clearActiveAgent,
      clearAgentInFlight,
      clearInFlight,
      clearStreamInFlight,
      clearToolActivity,
      clearWarnings,
      finalizeInFlight,
      finishStream,
      groupId,
      patchInFlight,
      pushToolActivity,
      pushWarning,
      qc,
      refreshActiveCount,
      setActiveAgent,
      token,
    ],
  )

  const cancel = useCallback(() => {
    const streamIds = Array.from(streamIdsRef.current.values())
    for (const ctrl of streamsRef.current.values()) {
      ctrl.abort()
    }
    streamsRef.current.clear()
    streamIdsRef.current.clear()
    setActiveStreamCount(0)
    if (groupId) {
      if (streamIds.length > 0) {
        for (const streamId of streamIds) {
          clearStreamInFlight(groupId, streamId)
        }
      } else {
        clearInFlight(groupId)
      }
    }
    window.setTimeout(invalidate, 700)
  }, [clearInFlight, clearStreamInFlight, groupId, invalidate])

  return { send, cancel, isStreaming: activeStreamCount > 0, activeStreamCount, error }
}
