/**
 * Resume a paused thread through the API v2 typed SSE contract.
 *
 * `resume()` POSTs to `/api/v2/threads/{threadId}/resume` and streams
 * continuation tokens. Each token is appended to the existing interrupted
 * message; the final `agent_message` event replaces that message locally
 * until query invalidation reconciles with the persisted row.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { z } from 'zod'

import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type { StreamEvent } from '@/lib/api-v2/types'
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
): Message {
  return {
    id: payload.message_id,
    group_id: groupId,
    thread_id: threadId,
    sender_type: 'agent',
    sender_id: payload.sender_id ?? payload.agent_id ?? null,
    message_type: 'text',
    content: payload.content ?? '',
    status: 'visible',
    refs: null,
    context_usage: null,
    reply_to_message_id: event.stream_id,
    created_at: nowIso(),
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
  const qc = useQueryClient()

  const [isStreaming, setIsStreaming] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const ctrlRef = useRef<AbortController | null>(null)

  useEffect(() => {
    return () => {
      ctrlRef.current?.abort()
    }
  }, [])

  const finish = useCallback(() => {
    setIsStreaming(false)
    ctrlRef.current = null
    if (messageId) endResume(messageId)
    if (groupId) {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'messages'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
    }
  }, [endResume, groupId, messageId, qc])

  const resume = useCallback(() => {
    if (!groupId || !threadId || !messageId || !token || isStreaming) return
    setError(null)
    setIsStreaming(true)
    startResume(messageId)

    const ctrl = openApiV2SseStream({
      url: `/api/v2/threads/${threadId}/resume`,
      body: {},
      token,
      handlers: {
        onEvent: (event) => {
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
              replaceMessage(groupId, buildAgentMessage(groupId, threadId, event, parsed.data))
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
            case 'user_message':
            case 'agent_start':
            case 'reasoning':
            case 'tool_call_start':
            case 'tool_call_result':
            case 'agent_silent':
            case 'waiting_for_user':
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
        onError: (err) => {
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
    appendToMessage,
    finish,
    groupId,
    isStreaming,
    messageId,
    replaceMessage,
    startResume,
    threadId,
    token,
  ])

  const cancel = useCallback(() => {
    ctrlRef.current?.abort()
    finish()
  }, [finish])

  return { resume, cancel, isStreaming, error }
}
