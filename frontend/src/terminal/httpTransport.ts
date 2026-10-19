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

import { apiOrigin, apiUrl } from '@/lib/runtime'
import { abortOnAuthChange, authFetch, expireAuthToken } from '@/lib/authFetch'
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

export type WireTerminalEvent =
  | { event: 'output'; data: { bytes_base64: string } }
  | { event: 'exit'; data: { code: number | null; signal: string | null } }
  | { event: 'error'; data: { code: string; message: string } }

export interface WireDescriptor {
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

export async function terminalRequest<T>(
  path: string,
  init: { method: 'GET' | 'POST' | 'DELETE'; body?: unknown; headers?: Record<string, string> },
): Promise<T | null> {
  let response: Response
  try {
    response = await authFetch(apiUrl(`/api/v2${path}`), {
      method: init.method,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${authToken()}`,
        ...init.headers,
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

export function wireEventToTerminalEvent(event: WireTerminalEvent): TerminalEvent {
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
  const clientId = crypto.randomUUID()
  const leases = new Map<string, number | null>()
  const streamIds = new Map<string, string>()
  const inputReleased = new Set<string>()
  const callbacks = new Map<string, (event: TerminalEvent) => void>()
  const streams = new Map<string, AbortController>()
  const writeTails = new Map<string, Promise<void>>()
  let identity: { token: string; origin: string } | undefined
  function request<T>(path: string, init: Parameters<typeof terminalRequest>[1]): Promise<T | null> {
    if (identity && (identity.token !== useAuthStore.getState().token || identity.origin !== apiOrigin())) {
      return Promise.reject(new TerminalTransportError('terminal.session_changed', 'The signed-in account or server changed.'))
    }
    return terminalRequest<T>(path, init)
  }
  function leaseHeaders(sessionId: string): Record<string, string> {
    const generation = leases.get(sessionId)
    if (!generation) throw new TerminalTransportError('terminal.lease_required', 'Another device controls this terminal. Reconnect to request input.')
    return { 'x-terminal-client': clientId, 'x-terminal-generation': String(generation) }
  }

  function openStream(
    sessionId: string,
    onEvent: (event: TerminalEvent) => void,
    lastEventId?: string,
    repaint = false,
  ): AbortController {
    const controller = new AbortController()
    const token = authToken()
    const renewal = setInterval(() => {
      const generation = leases.get(sessionId)
      if (controller.signal.aborted || inputReleased.has(sessionId)) return
      void request<{ generation: number | null }>(`/terminal/sessions/${sessionId}/lease`, {
        method: 'POST', body: { client_id: clientId, action: generation ? 'renew' : 'acquire', generation, stream: streamIds.get(sessionId) },
      }).then(result => { if (!controller.signal.aborted) {
        leases.set(sessionId, result?.generation ?? null)
        onEvent({ event: 'control', data: { writable: !!result?.generation } })
      } })
        .catch(() => { leases.set(sessionId, null) })
    }, 5_000)
    const cleanupAuth = abortOnAuthChange(controller, token)
    let recoveryQueued = false
    const recover = () => {
      if (controller.signal.aborted || document.hidden || !navigator.onLine || recoveryQueued) return
      recoveryQueued = true
      queueMicrotask(() => {
        if (controller.signal.aborted) return
        controller.abort()
        openStream(sessionId, onEvent, lastEventId)
      })
    }
    const cleanup = () => {
      clearInterval(renewal)
      cleanupAuth()
      document.removeEventListener('visibilitychange', recover)
      window.removeEventListener('online', recover)
      controller.signal.removeEventListener('abort', cleanup)
      if (streams.get(sessionId) === controller) streams.delete(sessionId)
    }
    controller.signal.addEventListener('abort', cleanup, { once: true })
    document.addEventListener('visibilitychange', recover)
    window.addEventListener('online', recover)
    // Tracked so `close` can stop the stream before the session is deleted; a
    // pending reconnect would otherwise resurrect a 404 loop.
    streams.set(sessionId, controller)

    void fetchEventSource(apiUrl(`/api/v2/terminal/sessions/${sessionId}/events${repaint ? '?repaint=true' : ''}`), {
      method: 'GET',
      headers: {
        Accept: 'text/event-stream',
        Authorization: `Bearer ${token}`,
        'x-terminal-client': clientId,
        ...(lastEventId ? { 'last-event-id': lastEventId } : {}),
      },
      signal: controller.signal,
      fetch: (_input, init) => {
        const headers = new Headers(init?.headers)
        headers.delete('last-event-id')
        if (lastEventId) headers.set('last-event-id', lastEventId)
        if (inputReleased.has(sessionId)) headers.set('x-terminal-observe', 'true')
        return authFetch(apiUrl(`/api/v2/terminal/sessions/${sessionId}/events${repaint ? '?repaint=true' : ''}`), { ...init, headers })
      },
      // The PTY keeps running while the tab is in the background, so the stream
      // has to keep draining or the reconnect replays a backlog.
      openWhenHidden: true,
      onopen: async (response) => {
        if (response.ok) {
          const generation = Number(response.headers.get('x-terminal-generation')) || null
          leases.set(sessionId, generation)
          const streamId = response.headers.get('x-terminal-stream')
          if (streamId) streamIds.set(sessionId, streamId)
          if (repaint) { lastEventId = response.headers.get('x-terminal-cursor') ?? lastEventId; repaint = false }
          onEvent({ event: 'control', data: { writable: generation !== null } })
          return
        }
        if (response.status === 401) expireAuthToken(token)
        throw errorFromEnvelope(
          response.status,
          await response.json().catch(() => null),
        )
      },
      onmessage: (message) => {
        if (controller.signal.aborted) return
        if (!message.data.trim()) return
        let parsed: WireTerminalEvent
        try {
          parsed = JSON.parse(message.data) as WireTerminalEvent
        } catch {
          throw new TerminalTransportError('terminal.invalid_event', 'The terminal returned an invalid event.')
        }
        const seq = Number(message.id)
        if (!Number.isSafeInteger(seq) || seq < 1) throw new TerminalTransportError('terminal.invalid_cursor', 'The terminal returned an invalid cursor.')
        if (seq <= Number(lastEventId ?? 0)) return
        if (seq > Number(lastEventId ?? 0) + 1) {
          throw new TerminalTransportError('terminal.output_gap', 'Terminal output exceeded the replay buffer. Reopen the terminal to start a new session.')
        }
        onEvent(wireEventToTerminalEvent(parsed))
        lastEventId = message.id
        if (parsed.event === 'exit' || parsed.event === 'error') controller.abort()
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
          controller.abort()
          throw cause
        }
      },
      onclose: () => {
        if (controller.signal.aborted) return
        throw new TypeError('Terminal stream closed before the shell exited')
      },
    }).catch(() => controller.abort())

    return controller
  }

  function stopStream(sessionId: string): void {
    streams.get(sessionId)?.abort()
    streams.delete(sessionId)
  }

  async function writeInput(sessionId: string, data: string, connection: AbortController | undefined): Promise<void> {
    for (const chunk of utf8InputChunks(toWellFormedUnicode(data))) {
      if (!connection || connection.signal.aborted || streams.get(sessionId) !== connection) throw new TerminalTransportError('terminal.disconnected', 'Terminal reconnected. Queued input was not resent.')
      await request(`/terminal/sessions/${sessionId}/input`, {
        method: 'POST',
        body: { data: chunk },
        headers: leaseHeaders(sessionId),
      })
    }
  }

  return {
    canAttach: true,
    async setInputControl(sessionId, acquire) {
      if (acquire) inputReleased.delete(sessionId)
      else inputReleased.add(sessionId)
      const lease = await request<{ generation: number | null }>(`/terminal/sessions/${sessionId}/lease`, { method: 'POST', body: {
        client_id: clientId, action: acquire ? 'acquire' : 'release', stream: streamIds.get(sessionId),
      } })
      leases.set(sessionId, lease?.generation ?? null)
      callbacks.get(sessionId)?.({ event: 'control', data: { writable: !!lease?.generation } })
    },
    async create(createRequest, onEvent) {
      identity ??= { token: authToken(), origin: apiOrigin() }
      const descriptor = createRequest.existingSessionId
        ? await request<WireDescriptor>(`/terminal/sessions/${encodeURIComponent(createRequest.existingSessionId)}`, { method: 'GET' })
        : await request<WireDescriptor>('/terminal/sessions', {
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
      const lease = await request<{ generation: number | null }>(`/terminal/sessions/${descriptor.session_id}/lease`, { method: 'POST', body: { client_id: clientId, action: 'acquire' } })
      leases.set(descriptor.session_id, lease?.generation ?? null)
      callbacks.set(descriptor.session_id, onEvent)
      onEvent({ event: 'control', data: { writable: !!lease?.generation } })
      openStream(descriptor.session_id, onEvent, undefined, !!createRequest.existingSessionId)
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
      const connection = streams.get(sessionId)
      const operation = previousTail === undefined
        ? writeInput(sessionId, data, connection)
        : previousTail.then(() => writeInput(sessionId, data, connection))
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
        headers: leaseHeaders(sessionId),
      })
    },

    async close(_conversationId, sessionId) {
      const writable = !!leases.get(sessionId)
      stopStream(sessionId)
      writeTails.delete(sessionId)
      if (!writable) return
      await request(`/terminal/sessions/${sessionId}`, { method: 'DELETE' })
    },

    async closeAll() {
      const owned = [...streams.keys()].filter(sessionId => leases.get(sessionId))
      for (const sessionId of [...streams.keys()]) {
        stopStream(sessionId)
      }
      writeTails.clear()
      await Promise.all(owned.map(sessionId => request(`/terminal/sessions/${sessionId}`, { method: 'DELETE' })))
    },
  }
}
