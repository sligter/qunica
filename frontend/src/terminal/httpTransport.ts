/**
 * Terminal transport for the web build.
 *
 * The desktop shell owns a native PTY; a browser tab cannot, so the server owns
 * it instead and this transport speaks to `/api/v2/terminal`. Output arrives on
 * an SSE stream, input and resizing are ordinary requests.
 *
 * A reconnect resumes from the last delivered frame id, so a dropped stream
 * repaints only what was missed rather than the whole scrollback.
 */

import { fetchEventSource } from '@microsoft/fetch-event-source'

import { apiUrl } from '@/lib/runtime'
import { useAuthStore } from '@/stores/authStore'

import {
  decodeBase64Bytes,
  toWellFormedUnicode,
  utf8InputChunks,
} from './encoding'
import {
  normalizeTerminalTransportError,
  TerminalTransportError,
  type TerminalTransport,
} from './transport'
import type { TerminalDescriptor, TerminalEvent } from './types'

type WireTerminalEvent =
  | { event: 'output'; data: { bytes_base64: string } }
  | { event: 'exit'; data: { code: number | null; signal: string | null } }
  | { event: 'error'; data: { code: string; message: string } }

interface WireDescriptor {
  session_id: string
  shell_name: string
  cwd: string
}

const SIGN_IN_REQUIRED_CODE = 'terminal.sign_in_required'

function authToken(): string {
  const token = useAuthStore.getState().token
  if (!token) {
    throw new TerminalTransportError(
      SIGN_IN_REQUIRED_CODE,
      'Sign in again to open a terminal.',
    )
  }
  return token
}

function errorFromEnvelope(status: number, payload: unknown): TerminalTransportError {
  const envelope = payload as { error?: { code?: unknown; message?: unknown } } | null
  const code = typeof envelope?.error?.code === 'string'
    ? envelope.error.code
    : `terminal.http_${status}`
  const message = typeof envelope?.error?.message === 'string'
    ? envelope.error.message
    : `Terminal request failed (${status})`
  return new TerminalTransportError(code, message)
}

async function request<T>(
  path: string,
  init: { method: 'POST' | 'DELETE'; body?: unknown },
): Promise<T | null> {
  let response: Response
  try {
    response = await fetch(apiUrl(`/api/v2${path}`), {
      method: init.method,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${authToken()}`,
      },
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
    })
  } catch (cause) {
    throw normalizeTerminalTransportError(cause, 'terminal.network_failed')
  }

  if (!response.ok) {
    const payload = await response.json().catch(() => null)
    throw errorFromEnvelope(response.status, payload)
  }
  if (response.status === 204) return null
  return (await response.json().catch(() => null)) as T | null
}

function wireEventToTerminalEvent(event: WireTerminalEvent): TerminalEvent {
  switch (event.event) {
    case 'output':
      return {
        event: 'output',
        data: { bytes: decodeBase64Bytes(event.data.bytes_base64) },
      }
    case 'exit':
      return { event: 'exit', data: event.data }
    case 'error':
      return { event: 'error', data: event.data }
  }
}

export function createHttpTerminalTransport(): TerminalTransport {
  const streams = new Map<string, AbortController>()
  const writeTails = new Map<string, Promise<void>>()

  function openStream(
    sessionId: string,
    onEvent: (event: TerminalEvent) => void,
  ): AbortController {
    const controller = new AbortController()
    // Tracked so `close` can stop the stream before the session is deleted; a
    // pending reconnect would otherwise resurrect a 404 loop.
    streams.set(sessionId, controller)

    void fetchEventSource(apiUrl(`/api/v2/terminal/sessions/${sessionId}/events`), {
      method: 'GET',
      headers: {
        Accept: 'text/event-stream',
        Authorization: `Bearer ${useAuthStore.getState().token ?? ''}`,
      },
      signal: controller.signal,
      // The PTY keeps running while the tab is in the background, so the stream
      // has to keep draining or the reconnect replays a backlog.
      openWhenHidden: true,
      onopen: async (response) => {
        if (response.ok) return
        throw errorFromEnvelope(
          response.status,
          await response.json().catch(() => null),
        )
      },
      onmessage: (message) => {
        if (!message.data.trim()) return
        let parsed: WireTerminalEvent
        try {
          parsed = JSON.parse(message.data) as WireTerminalEvent
        } catch {
          return
        }
        onEvent(wireEventToTerminalEvent(parsed))
      },
      onerror: (cause) => {
        if (controller.signal.aborted) return
        // Anything the server answered with is final: a closed session will not
        // reopen, and retrying a 401 just spins. Network blips fall through to
        // fetch-event-source's own backoff.
        if (cause instanceof TerminalTransportError) {
          onEvent({
            event: 'error',
            data: { code: cause.code, message: cause.message },
          })
          throw cause
        }
      },
      onclose: () => {
        // The server ends the stream after the shell exits; the exit frame has
        // already been delivered, so there is nothing to reconnect for.
        streams.delete(sessionId)
      },
    }).catch(() => undefined)

    return controller
  }

  function stopStream(sessionId: string): void {
    streams.get(sessionId)?.abort()
    streams.delete(sessionId)
  }

  async function writeInput(sessionId: string, data: string): Promise<void> {
    for (const chunk of utf8InputChunks(toWellFormedUnicode(data))) {
      await request(`/terminal/sessions/${sessionId}/input`, {
        method: 'POST',
        body: { data: chunk },
      })
    }
  }

  return {
    async create(createRequest, onEvent) {
      const descriptor = await request<WireDescriptor>('/terminal/sessions', {
        method: 'POST',
        body: {
          conversation_id: createRequest.conversationId,
          cwd: createRequest.cwd,
          cols: createRequest.cols,
          rows: createRequest.rows,
          shell: createRequest.shell,
        },
      })
      if (!descriptor?.session_id) {
        throw new TerminalTransportError(
          'terminal.create_failed',
          'The server did not return a terminal session.',
        )
      }
      openStream(descriptor.session_id, onEvent)
      return {
        sessionId: descriptor.session_id,
        shellName: descriptor.shell_name,
        cwd: descriptor.cwd,
      } satisfies TerminalDescriptor
    },

    // Serialized per session: two concurrent POSTs could otherwise interleave
    // keystrokes, and a shell reading a password would see them out of order.
    write(_conversationId, sessionId, data) {
      const previousTail = writeTails.get(sessionId)
      const operation = previousTail === undefined
        ? writeInput(sessionId, data)
        : previousTail.then(() => writeInput(sessionId, data))
      const storedTail = operation.catch(() => undefined)
      writeTails.set(sessionId, storedTail)
      void storedTail.finally(() => {
        if (writeTails.get(sessionId) === storedTail) {
          writeTails.delete(sessionId)
        }
      })
      return operation
    },

    async resize(_conversationId, sessionId, cols, rows) {
      await request(`/terminal/sessions/${sessionId}/resize`, {
        method: 'POST',
        body: { cols, rows },
      })
    },

    async close(_conversationId, sessionId) {
      stopStream(sessionId)
      writeTails.delete(sessionId)
      await request(`/terminal/sessions/${sessionId}`, { method: 'DELETE' })
    },

    async closeAll() {
      for (const sessionId of [...streams.keys()]) {
        stopStream(sessionId)
      }
      writeTails.clear()
      await request('/terminal/sessions', { method: 'DELETE' })
    },
  }
}
