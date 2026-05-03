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

export function useSendMessageStream(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const appendMessage = useMessageStore((s) => s.appendMessage)
  const patchInFlight = useMessageStore((s) => s.patchInFlight)
  const finalizeInFlight = useMessageStore((s) => s.finalizeInFlight)
  const clearInFlight = useMessageStore((s) => s.clearInFlight)
  const pushWarning = useMessageStore((s) => s.pushWarning)
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
  }, [groupId, qc])

  const send = useCallback(
    (content: string) => {
      if (!groupId || !token || isStreaming) return
      setError(null)
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
              return
            }
            if (event === 'warning') {
              pushWarning(groupId, data)
              return
            }
            if (event === 'done') {
              setIsStreaming(false)
              ctrlRef.current = null
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
      clearInFlight,
      finalizeInFlight,
      groupId,
      invalidate,
      isStreaming,
      patchInFlight,
      pushWarning,
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
