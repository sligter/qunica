import { openApiV2SseStream } from '@/lib/api-v2/sse'
import type { StreamEvent } from '@/lib/api-v2/types'
import { abortOnAuthChange } from '@/lib/authFetch'
import { LAN_ORIGIN } from '@/lib/androidSession'
import { apiOrigin } from '@/lib/runtime'
import { realtimeClient, type Command, type Receipt } from './client'

type Options = Parameters<typeof openApiV2SseStream>[0] & {
  observe?: { streamId: string; cursor: string; events: StreamEvent[] }
}

/** The paired Fetch tunnel remains SSE; direct clients share a ticket WS. */
export function openConversationStream(opts: Options): AbortController {
  if (apiOrigin() === LAN_ORIGIN) {
    if (!opts.observe) {
      const client = realtimeClient(opts.token)
      const body = opts.body as Record<string, unknown>
      const thread = opts.url.match(/^\/api\/v2\/threads\/([^/]+)\/resume$/)?.[1]
      if (thread) client.localContinuationThreads.add(thread)
      if (typeof body.client_request_id === 'string') client.localRuns.add(body.client_request_id)
      const release = () => { if (thread) client.localContinuationThreads.delete(thread) }
      const controller = openApiV2SseStream({ ...opts, handlers: { ...opts.handlers,
        onEvent: event => { client.localRuns.add(event.stream_id); release(); opts.handlers.onEvent(event) },
        onClose: () => { release(); opts.handlers.onClose?.() },
        onError: error => { release(); opts.handlers.onError?.(error) },
      } })
      controller.signal.addEventListener('abort', release, { once: true })
      return controller
    }
    let initialized = false
    const controller = openApiV2SseStream({ ...opts, url: opts.replayUrl?.(undefined) ?? opts.url,
      // SSE has no negative event IDs; the read-only URL already identifies a new run.
      initialMethod: 'GET', lastEventId: opts.observe.cursor === '-1'
        ? undefined : `${opts.observe.streamId}:${opts.observe.cursor}`,
      handlers: { ...opts.handlers, onOpen: () => {
        opts.handlers.onOpen?.()
        if (initialized) return
        initialized = true
        for (const event of opts.observe!.events) opts.handlers.onEvent(event)
        if (opts.observe!.events.some(event => event.kind === 'done' || event.kind === 'error')) { controller.abort(); opts.handlers.onClose?.() }
      } },
    })
    return controller
  }
  const ctrl = new AbortController()
  const cleanupAuth = abortOnAuthChange(ctrl, opts.token)
  const client = realtimeClient(opts.token)
  let unsubscribe: (() => void) | undefined
  let finished = false
  const cleanup = () => { finished = true; unsubscribe?.(); cleanupAuth() }
  ctrl.signal.addEventListener('abort', cleanup, { once: true })

  async function start() {
    if (opts.observe) {
      opts.handlers.onOpen?.()
      for (const event of opts.observe.events) {
        if (finished) return
        opts.handlers.onEvent(event)
        if (event.kind === 'done' || event.kind === 'error') { cleanup(); opts.handlers.onClose?.(); return }
      }
      subscribe(opts.observe.streamId, opts.observe.cursor)
      return
    }
    const body = opts.body as Record<string, unknown>
    const message = opts.url.match(/^\/api\/v2\/(groups|direct-chats)\/([^/]+)\/messages\/stream$/)
    const resume = opts.url.match(/^\/api\/v2\/threads\/([^/]+)\/resume$/)
    let command: Command
    if (message) command = {
      request_id: String(body.client_request_id), name: 'message.send',
      target: { conversation_id: message[2], thread_id: body.thread_id ?? null }, payload: body,
    }
    else if (resume) command = {
      request_id: typeof body.request_id === 'string' ? body.request_id : crypto.randomUUID(),
      name: body.approval ? 'approval.resolve' : 'thread.resume', target: { thread_id: resume[1] },
      payload: body.approval ? { ...(body.approval as Record<string, unknown>) } : {},
    }
    else throw new Error('Unsupported realtime command endpoint')
    if (message) client.localRuns.add(command.request_id)
    if (resume && body.approval) {
      const snapshot = await client.request<{ pending_approvals: { tool_call_id: string; action_hash: string; version: number }[] }>(`/threads/${resume[1]}/snapshot`)
      const approval = snapshot.pending_approvals.find(item => item.tool_call_id === (body.approval as { tool_call_id: string }).tool_call_id)
      if (!approval) throw new Error('This approval is no longer pending. Refresh the conversation.')
      command.payload.expected_version = approval.version
      command.payload.action_hash = approval.action_hash
    }
    if (finished) return
    if (resume) client.localContinuationThreads.add(resume[1])
    let receipt: Receipt
    try { receipt = await client.command(command) }
    finally { if (resume) client.localContinuationThreads.delete(resume[1]) }
    if (receipt.stream_id) client.localRuns.add(receipt.stream_id)
    if (finished) return
    opts.handlers.onOpen?.()
    if (!receipt.stream_id) { cleanup(); opts.handlers.onClose?.(); return }
    subscribe(receipt.stream_id, opts.lastEventId?.slice(opts.lastEventId.lastIndexOf(':') + 1) ?? '-1')
  }

  function subscribe(streamId: string, cursor: string) {
    unsubscribe = client.subscribe(`run:${streamId}`, cursor, frame => {
      if (finished) return
      // Note: strict domain parsers must not receive WS-only cursor/scope/type fields.
      const { stream_id, seq, event_id, kind, payload } = frame as unknown as StreamEvent
      const event = { stream_id, seq, event_id, kind, payload }
      opts.handlers.onEvent(event)
      if (event.kind === 'done' || event.kind === 'error') { cleanup(); opts.handlers.onClose?.() }
    }, error => {
      if (finished) return
      cleanup()
      void Promise.resolve(opts.handlers.onRecover?.(ctrl.signal)).then(active => {
        if (active === false) opts.handlers.onClose?.()
        else opts.handlers.onDisconnect?.(error)
      }).catch(err => opts.handlers.onError?.(err))
    })
  }
  void Promise.resolve().then(start).catch(error => { if (!finished) { cleanup(); opts.handlers.onError?.(error) } })
  return ctrl
}
