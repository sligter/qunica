import { fetchEventSource } from '@microsoft/fetch-event-source'
import { apiUrl } from '@/lib/runtime'
import { abortOnAuthChange, authFetch, expireAuthToken } from '@/lib/authFetch'
import { isAbortError, isNetworkError, isRetryableHttpStatus, MAX_RETRY_ATTEMPTS, retryDelayMs } from './retry'
import type { StreamEvent } from './types'

export interface ApiV2SseHandlers {
  onEvent: (event: StreamEvent) => void
  onOpen?: () => void
  onRetry?: (attempt: number, delayMs: number) => void
  onError?: (err: unknown) => void
  onClose?: () => void
  /** Refresh server state; false means the turn is already terminal. */
  onRecover?: (signal: AbortSignal) => Promise<boolean>
  /** Disconnected is not a failed server turn. Keep its local cursor/draft. */
  onDisconnect?: (err: unknown) => void
}

class FatalSseError extends Error {}
class RetryableSseError extends Error {}

export class SseRetryExhaustedError extends Error {
  constructor(cause: unknown) {
    super('SSE retry policy exhausted after ' + MAX_RETRY_ATTEMPTS + ' attempts: ' + errorMessage(cause))
    this.name = 'SseRetryExhaustedError'
  }
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

export function openApiV2SseStream(opts: {
  url: string
  body: unknown
  token: string
  lastEventId?: string
  /** Read-only endpoint for retries. Missing identity must never re-execute a POST. */
  replayUrl?: (lastEventId: string | undefined) => string | null
  handlers: ApiV2SseHandlers
}): AbortController {
  const ctrl = new AbortController()
  const cleanupAuth = abortOnAuthChange(ctrl, opts.token)
  let connection: AbortController | undefined
  let generation = 0
  let retryAttempts = 0
  let terminalEventSeen = false
  let firstRequest = true
  let lastEventId = opts.lastEventId
  let recovering = false
  let finished = false
  const appliedSequence = new Map<string, number>()

  function cleanup() {
    finished = true
    connection?.abort()
    cleanupAuth()
    ctrl.signal.removeEventListener('abort', cleanup)
    document.removeEventListener('visibilitychange', recover)
    window.removeEventListener('online', recover)
    window.removeEventListener('qunica:reconnect', recover)
  }

  function close() {
    cleanup()
    opts.handlers.onClose?.()
  }

  function fail(error: unknown) {
    cleanup()
    opts.handlers.onError?.(error)
  }

  function disconnect(error: unknown) {
    if (opts.replayUrl && opts.handlers.onDisconnect) opts.handlers.onDisconnect(error)
    else fail(error)
  }

  async function recover() {
    if (!opts.replayUrl || finished || ctrl.signal.aborted || recovering || document.hidden || !navigator.onLine) return
    recovering = true
    ++generation
    connection?.abort()
    try {
      const keepStreaming = await opts.handlers.onRecover?.(ctrl.signal) ?? true
      if (finished || ctrl.signal.aborted) return
      if (!keepStreaming) { close(); return }
      retryAttempts = 0
      connect()
    } catch (error) {
      if (!finished && !ctrl.signal.aborted) disconnect(error)
    } finally {
      recovering = false
    }
  }

  function connect() {
    const currentGeneration = ++generation
    const current = new AbortController()
    connection = current
    const stale = () => finished || ctrl.signal.aborted || currentGeneration !== generation
    const headers: Record<string, string> = {
      Accept: 'text/event-stream',
      'Content-Type': 'application/json',
      Authorization: 'Bearer ' + opts.token,
    }
    if (lastEventId) headers['last-event-id'] = lastEventId

    void fetchEventSource(apiUrl(opts.url), {
      method: 'POST', headers, body: JSON.stringify(opts.body), signal: current.signal,
      openWhenHidden: true,
      fetch: async (input, init) => {
        if (stale()) throw new DOMException('Stream superseded', 'AbortError')
        const isFirst = firstRequest
        firstRequest = false
        // The library advances its cursor before onmessage applies the event.
        const nextHeaders = new Headers(init?.headers)
        nextHeaders.delete('last-event-id')
        if (lastEventId) nextHeaders.set('last-event-id', lastEventId)
        if (!isFirst && opts.replayUrl) {
          const url = opts.replayUrl(lastEventId)
          if (!url) {
            await opts.handlers.onRecover?.(ctrl.signal)
            throw new FatalSseError('Stream identity unavailable. State refreshed; execution was not repeated.')
          }
          return authFetch(apiUrl(url), { ...init, method: 'GET', body: undefined, headers: nextHeaders })
        }
        return authFetch(input, { ...init, headers: nextHeaders })
      },
      onopen: async response => {
        if (stale()) throw new DOMException('Stream superseded', 'AbortError')
        if (!response.ok) {
          if (response.status === 401) expireAuthToken(opts.token)
          // A send may be accepted before the first event is durable.
          if (response.status === 404 && opts.replayUrl && !firstRequest) {
            if (lastEventId) lastEventId = undefined
            throw new RetryableSseError('Stream replay is not available yet')
          }
          if (isRetryableHttpStatus(response.status)) throw new RetryableSseError('SSE open failed: ' + response.status)
          throw new FatalSseError('SSE open failed: ' + response.status)
        }
        opts.handlers.onOpen?.()
      },
      onmessage: msg => {
        if (stale() || !msg.data.trim()) return
        let event: StreamEvent
        try { event = JSON.parse(msg.data) as StreamEvent }
        catch (error) { throw new FatalSseError('SSE event was not valid JSON: ' + errorMessage(error)) }
        if (!event.stream_id || !event.event_id || !Number.isSafeInteger(event.seq)) {
          throw new FatalSseError('SSE event has no valid stream cursor')
        }
        if (event.seq <= (appliedSequence.get(event.stream_id) ?? -1)) return
        try { opts.handlers.onEvent(event) }
        catch (error) { throw new RetryableSseError('SSE event handler failed: ' + errorMessage(error)) }
        lastEventId = event.event_id
        appliedSequence.set(event.stream_id, event.seq)
        terminalEventSeen = event.kind === 'done' || event.kind === 'error'
        retryAttempts = 0
        if (terminalEventSeen) close()
      },
      onerror: error => {
        if (stale() || isAbortError(error)) throw error
        if (error instanceof FatalSseError || (!(error instanceof RetryableSseError) && !isNetworkError(error))) {
          fail(error)
          throw error
        }
        retryAttempts += 1
        if (retryAttempts > MAX_RETRY_ATTEMPTS) {
          const exhausted = new SseRetryExhaustedError(error)
          disconnect(exhausted)
          throw exhausted
        }
        const delay = retryDelayMs(retryAttempts)
        opts.handlers.onRetry?.(retryAttempts, delay)
        return delay
      },
      onclose: () => {
        if (stale()) return
        if (terminalEventSeen) { close(); return }
        throw new RetryableSseError('SSE closed before a terminal event')
      },
    }).catch(() => undefined)
  }

  ctrl.signal.addEventListener('abort', cleanup, { once: true })
  if (opts.replayUrl) {
    document.addEventListener('visibilitychange', recover)
    window.addEventListener('online', recover)
    window.addEventListener('qunica:reconnect', recover)
  }
  connect()
  return ctrl
}
