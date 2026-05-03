/**
 * Resume a paused thread.
 *
 * `resume()` POSTs to `/threads/{threadId}/resume` and streams continuation
 * tokens. Each token is APPENDED to the existing interrupted message
 * (keyed by `messageId`) in the store; the final `agent_message` event
 * replaces that message with its persisted form (status flips back to
 * `visible`).
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { openSseStream } from '@/lib/sse'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface TokenPayload {
  agent_id: string
  delta: string
}

function safeJson<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    return null
  }
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
    }
  }, [endResume, groupId, messageId, qc])

  const resume = useCallback(() => {
    if (!groupId || !threadId || !messageId || !token || isStreaming) return
    setError(null)
    setIsStreaming(true)
    startResume(messageId)

    const ctrl = openSseStream({
      url: `/api/v1/threads/${threadId}/resume`,
      body: {},
      token,
      handlers: {
        onEvent: (event, data) => {
          if (event === 'token') {
            const payload = safeJson<TokenPayload>(data)
            if (payload?.delta) appendToMessage(groupId, messageId, payload.delta)
            return
          }
          if (event === 'agent_message') {
            const msg = safeJson<Message>(data)
            if (msg) replaceMessage(groupId, msg)
            return
          }
          if (event === 'done') {
            finish()
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
