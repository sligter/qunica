import { fetchEventSource } from '@microsoft/fetch-event-source'

import { apiUrl } from '@/lib/runtime'

import {
  isAbortError,
  isNetworkError,
  isRetryableHttpStatus,
  MAX_RETRY_ATTEMPTS,
  retryDelayMs,
} from './retry'
import type { StreamEvent } from './types'

export interface ApiV2SseHandlers {
  onEvent: (event: StreamEvent) => void
  onRetry?: (attempt: number, delayMs: number) => void
  onError?: (err: unknown) => void
  onClose?: () => void
}

class FatalSseError extends Error {}
class RetryableSseError extends Error {}

export class SseRetryExhaustedError extends Error {
  constructor(cause: unknown) {
    super(
      `SSE retry policy exhausted after ${MAX_RETRY_ATTEMPTS} attempts: ${errorMessage(cause)}`,
    )
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
  handlers: ApiV2SseHandlers
}): AbortController {
  const ctrl = new AbortController()
  let retryAttempts = 0
  let terminalEventSeen = false
  const headers: Record<string, string> = {
    Accept: 'text/event-stream',
    'Content-Type': 'application/json',
    Authorization: `Bearer ${opts.token}`,
  }

  if (opts.lastEventId) {
    // fetch-event-source keeps this header updated from every subsequent msg.id.
    headers['last-event-id'] = opts.lastEventId
  }

  void fetchEventSource(apiUrl(opts.url), {
    method: 'POST',
    headers,
    body: JSON.stringify(opts.body),
    signal: ctrl.signal,
    openWhenHidden: true,
    onopen: async (response) => {
      if (!response.ok) {
        if (isRetryableHttpStatus(response.status)) {
          throw new RetryableSseError(`SSE open failed: ${response.status}`)
        }
        throw new FatalSseError(`SSE open failed: ${response.status}`)
      }
    },
    onmessage: (msg) => {
      let parsed: StreamEvent
      try {
        parsed = JSON.parse(msg.data) as StreamEvent
      } catch (err) {
        throw new FatalSseError(`SSE event was not valid JSON: ${errorMessage(err)}`)
      }
      terminalEventSeen = parsed.kind === 'done' || parsed.kind === 'error'
      try {
        opts.handlers.onEvent(parsed)
      } catch (err) {
        throw new RetryableSseError(`SSE event handler failed: ${errorMessage(err)}`)
      }
      retryAttempts = 0
    },
    onerror: (err) => {
      if (ctrl.signal.aborted || isAbortError(err)) {
        return
      }
      if (
        err instanceof FatalSseError ||
        (!(err instanceof RetryableSseError) && !isNetworkError(err))
      ) {
        opts.handlers.onError?.(err)
        throw err
      }
      retryAttempts += 1
      if (retryAttempts > MAX_RETRY_ATTEMPTS) {
        const fatal = new SseRetryExhaustedError(err)
        opts.handlers.onError?.(fatal)
        throw fatal
      }
      const delayMs = retryDelayMs(retryAttempts)
      opts.handlers.onRetry?.(retryAttempts, delayMs)
      return delayMs
    },
    onclose: () => {
      if (ctrl.signal.aborted || terminalEventSeen) {
        opts.handlers.onClose?.()
        return
      }
      throw new RetryableSseError('SSE closed before a terminal event')
    },
  }).catch(() => undefined)

  return ctrl
}
