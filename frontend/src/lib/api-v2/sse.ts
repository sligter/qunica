import { fetchEventSource } from '@microsoft/fetch-event-source'

import { apiUrl } from '@/lib/runtime'

import type { StreamEvent } from './types'

export interface ApiV2SseHandlers {
  onEvent: (event: StreamEvent) => void
  onError?: (err: unknown) => void
  onClose?: () => void
}

export function openApiV2SseStream(opts: {
  url: string
  body: unknown
  token: string
  lastEventId?: string
  handlers: ApiV2SseHandlers
}): AbortController {
  const ctrl = new AbortController()
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
        throw new Error(`SSE open failed: ${response.status}`)
      }
    },
    onmessage: (msg) => {
      const parsed = JSON.parse(msg.data) as StreamEvent
      opts.handlers.onEvent(parsed)
    },
    onerror: (err) => {
      opts.handlers.onError?.(err)
      throw err
    },
    onclose: () => {
      opts.handlers.onClose?.()
    },
  })

  return ctrl
}
