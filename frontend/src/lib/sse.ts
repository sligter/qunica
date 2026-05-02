/**
 * SSE client built on `@microsoft/fetch-event-source`.
 *
 * Native EventSource cannot attach an `Authorization: Bearer` header; we use
 * fetch-event-source which can. Returns an `AbortController` so the caller
 * can cancel mid-stream (component unmount, user-cancel).
 */

import { fetchEventSource } from '@microsoft/fetch-event-source'

export interface SseHandlers {
  onEvent: (event: string, data: string) => void
  onError?: (err: unknown) => void
  onClose?: () => void
}

interface OpenSseOptions {
  url: string
  body: unknown
  token: string
  handlers: SseHandlers
}

export function openSseStream(opts: OpenSseOptions): AbortController {
  const ctrl = new AbortController()

  void fetchEventSource(opts.url, {
    method: 'POST',
    headers: {
      Accept: 'text/event-stream',
      'Content-Type': 'application/json',
      Authorization: `Bearer ${opts.token}`,
    },
    body: JSON.stringify(opts.body),
    signal: ctrl.signal,
    // Don't auto-reconnect on close — for messaging, "done" is final.
    openWhenHidden: true,
    onopen: async (response) => {
      if (!response.ok) {
        throw new Error(`SSE open failed: ${response.status}`)
      }
    },
    onmessage: (msg) => {
      opts.handlers.onEvent(msg.event || 'message', msg.data)
    },
    onerror: (err) => {
      opts.handlers.onError?.(err)
      // Throwing terminates the connection (the lib otherwise retries).
      throw err
    },
    onclose: () => {
      opts.handlers.onClose?.()
    },
  })

  return ctrl
}
