/**
 * Stream-send a message to a group.
 *
 * Returns `{ send, cancel, isStreaming, error }`. `send(content)` opens an
 * SSE connection to `/groups/{id}/messages/stream`, dispatches every event
 * into the messageStore (user_message → token → agent_message → done), and
 * resolves when the `done` event arrives or the stream errors.
 *
 * Cancels any in-flight stream when the component unmounts. After cancel,
 * invalidates the messages query so the persisted "interrupted" state from
 * the backend's `finally` block becomes visible after the next refetch.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { openSseStream } from '@/lib/sse'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ActiveAgent, ToolActivity, ToolActivityStatus } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface TokenPayload {
  agent_id: string
  delta: string
}

interface AgentErrorPayload {
  agent_id: string
  display_name: string
  error: string
}

interface WaitingForUserPayload {
  message?: string
}

interface ToolCallPayload {
  agent_id?: string
  display_name?: string
  tool_call_id?: string
  tool_name?: string
  status?: ToolActivityStatus
  args_summary?: string
  result_summary?: string
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

export function useSendMessageStream(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const appendMessage = useMessageStore((s) => s.appendMessage)
  const patchInFlight = useMessageStore((s) => s.patchInFlight)
  const finalizeInFlight = useMessageStore((s) => s.finalizeInFlight)
  const clearInFlight = useMessageStore((s) => s.clearInFlight)
  const clearAgentInFlight = useMessageStore((s) => s.clearAgentInFlight)
  const setActiveAgent = useMessageStore((s) => s.setActiveAgent)
  const clearActiveAgent = useMessageStore((s) => s.clearActiveAgent)
  const pushWarning = useMessageStore((s) => s.pushWarning)
  const clearWarnings = useMessageStore((s) => s.clearWarnings)
  const pushToolActivity = useMessageStore((s) => s.pushToolActivity)
  const clearToolActivity = useMessageStore((s) => s.clearToolActivity)
  const qc = useQueryClient()

  const [isStreaming, setIsStreaming] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const ctrlRef = useRef<AbortController | null>(null)

  useEffect(() => {
    return () => {
      ctrlRef.current?.abort()
    }
  }, [])

  const invalidate = useCallback(() => {
    if (!groupId) return
    void qc.invalidateQueries({ queryKey: ['groups', groupId, 'messages'] })
    void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
  }, [groupId, qc])

  const send = useCallback(
    (content: string) => {
      if (!groupId || !token || isStreaming) return
      setError(null)
      clearWarnings(groupId)
      clearToolActivity(groupId)
      setIsStreaming(true)

      const ctrl = openSseStream({
        url: `/api/v1/groups/${groupId}/messages/stream`,
        body: { content },
        token,
        handlers: {
          onEvent: (event, data) => {
            if (event === 'user_message') {
              const msg = safeJson<Message>(data)
              if (msg) appendMessage(groupId, msg)
              return
            }
            if (event === 'agent_start') {
              const info = safeJson<ActiveAgent>(data)
              if (info) setActiveAgent(groupId, info)
              return
            }
            if (event === 'token') {
              const payload = safeJson<TokenPayload>(data)
              if (payload?.agent_id && payload.delta) {
                patchInFlight(groupId, payload.agent_id, payload.delta)
              }
              return
            }
            if (event === 'agent_message') {
              const msg = safeJson<Message>(data)
              if (msg) finalizeInFlight(groupId, msg)
              void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
              return
            }
            if (event === 'agent_silent') {
              const info = safeJson<{ agent_id: string }>(data)
              if (info?.agent_id) clearAgentInFlight(groupId, info.agent_id)
              return
            }
            if (event === 'tool_call_start' || event === 'tool_call_result') {
              const payload = safeJson<ToolCallPayload>(data)
              if (payload?.tool_call_id) {
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
            if (event === 'silence') {
              pushWarning(groupId, 'No one replied')
              return
            }
            if (event === 'waiting_for_user') {
              const payload = safeJson<WaitingForUserPayload>(data)
              pushWarning(groupId, payload?.message || 'Waiting for your input')
              clearActiveAgent(groupId)
              return
            }
            if (event === 'agent_error') {
              const err = safeJson<AgentErrorPayload>(data)
              if (err) {
                pushWarning(groupId, `Agent "${err.display_name}" failed: ${err.error}`)
              }
              return
            }
            if (event === 'warning') {
              pushWarning(groupId, data)
              return
            }
            if (event === 'done') {
              clearActiveAgent(groupId)
              setIsStreaming(false)
              ctrlRef.current = null
              window.setTimeout(() => clearToolActivity(groupId), 4_000)
              invalidate()
            }
          },
          onError: (err) => {
            setError(err instanceof Error ? err.message : String(err))
            setIsStreaming(false)
            clearInFlight(groupId)
            ctrlRef.current = null
            invalidate()
          },
          onClose: () => {
            setIsStreaming(false)
            ctrlRef.current = null
            invalidate()
          },
        },
      })
      ctrlRef.current = ctrl
    },
    [
      appendMessage,
      clearActiveAgent,
      clearAgentInFlight,
      clearInFlight,
      clearToolActivity,
      clearWarnings,
      finalizeInFlight,
      groupId,
      invalidate,
      isStreaming,
      patchInFlight,
      pushToolActivity,
      pushWarning,
      qc,
      setActiveAgent,
      token,
    ],
  )

  const cancel = useCallback(() => {
    ctrlRef.current?.abort()
    ctrlRef.current = null
    setIsStreaming(false)
    if (groupId) clearInFlight(groupId)
    // Backend persists the interrupted message and commits inside an
    // asyncio.shield block after CancelledError. Give it a moment to
    // land before we refetch — otherwise the refetch hits the DB before
    // the shielded commit and the bubble appears to vanish.
    window.setTimeout(invalidate, 700)
  }, [clearInFlight, groupId, invalidate])

  return { send, cancel, isStreaming, error }
}
