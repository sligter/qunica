import { fetchEventSource } from '@microsoft/fetch-event-source'

import { apiUrl } from '@/lib/runtime'

import type { StreamEvent } from './types'

const MAX_RETRY_ATTEMPTS = 5
const BASE_RETRY_MS = 1_000
const MAX_RETRY_MS = 5_000

export interface ApiV2SseHandlers {
  onEvent: (event: StreamEvent) => void
  onError?: (err: unknown) => void
  onClose?: () => void
}

class FatalSseError extends Error {}

function isAbortError(err: unknown): boolean {
  return err instanceof DOMException && err.name === 'AbortError'
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
    headers['Last-Event-ID'] = opts.lastEventId
  }

  void fetchEventSource(apiUrl(opts.url), {
    method: 'POST',
    headers,
    body: JSON.stringify(opts.body),
    signal: ctrl.signal,
    openWhenHidden: true,
    onopen: async (response) => {
      if (!response.ok) {
        throw new FatalSseError(`SSE open failed: ${response.status}`)
      }
      retryAttempts = 0
    },
    onmessage: (msg) => {
      let parsed: StreamEvent
      try {
        parsed = JSON.parse(msg.data) as StreamEvent
      } catch (err) {
        throw new FatalSseError(`SSE event was not valid JSON: ${errorMessage(err)}`)
      }
      terminalEventSeen = parsed.kind === 'done' || parsed.kind === 'error'
      opts.handlers.onEvent(parsed)
    },
    onerror: (err) => {
      if (ctrl.signal.aborted || isAbortError(err)) {
        return
      }
      if (err instanceof FatalSseError) {
        opts.handlers.onError?.(err)
        throw err
      }
      retryAttempts += 1
      if (retryAttempts > MAX_RETRY_ATTEMPTS) {
        const fatal = new FatalSseError(
          `SSE retry policy exhausted after ${MAX_RETRY_ATTEMPTS} attempts: ${errorMessage(err)}`,
        )
        opts.handlers.onError?.(fatal)
        throw fatal
      }
      return Math.min(BASE_RETRY_MS * retryAttempts, MAX_RETRY_MS)
    },
    onclose: () => {
      if (ctrl.signal.aborted || terminalEventSeen) {
        opts.handlers.onClose?.()
        return
      }
      throw new Error('SSE closed before a terminal event')
    },
  })

  return ctrl
}
