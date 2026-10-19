import { ApiError } from '@/lib/api-v2/client'
import { retryDelayMs } from '@/lib/api-v2/retry'
import { abortOnAuthChange, authFetch } from '@/lib/authFetch'
import { apiOrigin } from '@/lib/runtime'

export const REALTIME_PROTOCOL = 'qunica.realtime.v2'
export type RealtimeFrame = Record<string, unknown> & { type: string; scope?: string; cursor?: string }
export interface Command {
  request_id: string
  name: 'message.send' | 'thread.resume' | 'approval.resolve' | 'turn.cancel' | 'turn.acknowledge_stopped'
  target: Record<string, unknown>
  payload: Record<string, unknown>
}
export interface Receipt {
  request_id: string
  status: string
  stream_id?: string
  turn_id?: string
  thread_id?: string
  job_id?: string
}
interface Listener {
  cursor: string
  apply: (event: RealtimeFrame) => void
  resync: (error: Error) => void
}
interface Pending {
  resolve: (receipt: Receipt) => void
  reject: (error: Error) => void
  timer: ReturnType<typeof setTimeout>
}

function protocolError(code: string, message = code): ApiError {
  return new ApiError(409, code, message)
}
export function validCursor(cursor: unknown, run = false): cursor is string {
  return typeof cursor === 'string' && ((run && cursor === '-1') || /^(0|[1-9]\d{0,18})$/.test(cursor))
    && BigInt(cursor) <= 9223372036854775807n
}

const TEXT_KINDS = new Set(['token', 'reasoning'])

function textDelta(payload: unknown): string | undefined {
  if (!payload || typeof payload !== 'object') return undefined
  const { delta, text } = payload as { delta?: unknown; text?: unknown }
  const value = delta ?? text
  return typeof value === 'string' ? value : undefined
}

/**
 * Merge consecutive token or reasoning deltas from the same run and agent, so a
 * replayed backlog costs one store write per run of text instead of one per delta.
 */
export function coalesceTextEvents(events: RealtimeFrame[]): RealtimeFrame[] {
  const merged: RealtimeFrame[] = []
  for (const event of events) {
    const previous = merged.at(-1)
    const text = textDelta(event.payload)
    const previousText = previous && textDelta(previous.payload)
    if (
      previous && text !== undefined && previousText !== undefined
      && TEXT_KINDS.has(String(event.kind)) && previous.kind === event.kind
      && previous.stream_id === event.stream_id
      && (previous.payload as { agent_id?: unknown }).agent_id === (event.payload as { agent_id?: unknown }).agent_id
    ) {
      const joined = previousText + text
      merged[merged.length - 1] = { ...event, payload: { ...(event.payload as object), delta: joined, text: joined } }
      continue
    }
    merged.push(event)
  }
  return merged
}

/** One connection per origin/session. Reconnects only replay subscriptions and read receipts. */
export class RealtimeClient {
  readonly controller = new AbortController()
  readonly localRuns = new Set<string>()
  readonly localContinuationThreads = new Set<string>()
  private socket?: WebSocket
  private serverId?: string
  private ready?: Promise<void>
  private readyResolve?: () => void
  private readyReject?: (error: Error) => void
  private authenticated = false
  private retry = 0
  private reconnectTimer?: ReturnType<typeof setTimeout>
  private handshakeTimer?: ReturnType<typeof setTimeout>
  private heartbeatTimer?: ReturnType<typeof setTimeout>
  private listeners = new Map<string, Set<Listener>>()
  private pending = new Map<string, Pending>()
  private readonly cleanupAuth: () => void
  private fatal?: Error
  private stateListeners = new Set<(status: 'connecting' | 'connected' | 'error') => void>()
  observeConnection(listener: (status: 'connecting' | 'connected' | 'error') => void): () => void {
    this.stateListeners.add(listener)
    listener(this.fatal ? 'error' : this.authenticated ? 'connected' : 'connecting')
    return () => { this.stateListeners.delete(listener) }
  }

  constructor(readonly origin: string, readonly token: string) {
    this.cleanupAuth = abortOnAuthChange(this.controller, token)
    this.controller.signal.addEventListener('abort', () => this.dispose(), { once: true })
    window.addEventListener('online', this.reconnect)
    window.addEventListener('qunica:reconnect', this.reconnect)
    document.addEventListener('visibilitychange', this.foreground)
  }

  async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    if (this.controller.signal.aborted || apiOrigin() !== this.origin) throw new DOMException('Session changed', 'AbortError')
    const response = await authFetch(`${this.origin}/api/v2${path}`, {
      ...init, signal: AbortSignal.any([this.controller.signal, AbortSignal.timeout(10_000)]),
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${this.token}`, ...init.headers },
    })
    const value = await response.json()
    if (this.controller.signal.aborted || apiOrigin() !== this.origin) throw new DOMException('Session changed', 'AbortError')
    if (!response.ok) throw new ApiError(response.status, value.error?.code ?? 'http_error', value.error?.message ?? `HTTP ${response.status}`, value.error?.details)
    return value as T
  }

  subscribe(scope: string, cursor: string, apply: Listener['apply'], resync: Listener['resync']): () => void {
    if (!validCursor(cursor, scope.startsWith('run:'))) throw protocolError('invalid_cursor')
    const listener = { cursor, apply, resync }
    const set = this.listeners.get(scope) ?? new Set<Listener>()
    set.add(listener)
    this.listeners.set(scope, set)
    if (this.authenticated) this.subscribeScope(scope)
    else void this.connect().catch(error => { if (this.fatal) resync(error) })
    return () => {
      set.delete(listener)
      if (set.size) return
      this.listeners.delete(scope)
      if (this.authenticated) this.send({ type: 'unsubscribe', scope })
    }
  }

  async command(command: Command): Promise<Receipt> {
    await this.connect()
    if (this.pending.size >= 256 || this.pending.has(command.request_id)) throw protocolError('command_pending')
    return new Promise<Receipt>((resolve, reject) => {
      const timer = setTimeout(() => { void this.readReceipt(command.request_id) }, 15_000)
      this.pending.set(command.request_id, { resolve, reject, timer })
      try { this.send({ type: 'command', ...command }) }
      catch (error) { this.settle(command.request_id, undefined, error as Error) }
    })
  }

  private settle(id: string, result?: Receipt, error?: Error) {
    const pending = this.pending.get(id)
    if (!pending) return
    this.pending.delete(id)
    clearTimeout(pending.timer)
    if (error) pending.reject(error)
    else pending.resolve(result!)
  }

  private async readReceipt(id: string) {
    if (!this.pending.has(id)) return
    try { this.settle(id, await this.request<Receipt>(`/commands/${encodeURIComponent(id)}`)) }
    catch (error) {
      // Note: a missing receipt after a lost response is uncertain acceptance;
      // only an explicit user retry may send that command again.
      this.settle(id, undefined, error instanceof ApiError && error.status === 404
        ? protocolError('command_unconfirmed', 'Command acceptance could not be confirmed. Refresh before retrying.') : error as Error)
    }
  }

  private send(frame: Record<string, unknown>) {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) throw protocolError('realtime_disconnected')
    const raw = JSON.stringify(frame)
    if (new TextEncoder().encode(raw).length > 64 * 1024 || this.socket.bufferedAmount > 4 * 1024 * 1024) throw protocolError('realtime_backpressure')
    this.socket.send(raw)
  }

  private subscribeScope(scope: string) {
    const cursors = [...(this.listeners.get(scope) ?? [])].map(listener => listener.cursor)
    if (!cursors.length) return
    const cursor = cursors.reduce((a, b) => BigInt(a) < BigInt(b) ? a : b)
    this.send({ type: 'subscribe', scope, cursor, server_id: this.serverId })
  }

  private connect(): Promise<void> {
    if (this.fatal) return Promise.reject(this.fatal)
    if (this.controller.signal.aborted) return Promise.reject(new DOMException('Session changed', 'AbortError'))
    if (this.authenticated) return Promise.resolve()
    if (this.ready) return this.ready
    clearTimeout(this.reconnectTimer)
    this.ready = new Promise<void>((resolve, reject) => { this.readyResolve = resolve; this.readyReject = reject })
    this.handshakeTimer = setTimeout(() => this.socket?.close(), 10_000)
    void this.open().catch(error => this.disconnected(error))
    return this.ready
  }

  private async open() {
    const ticket = await this.request<{ ticket: string; server_id: string; protocol: string }>('/realtime/tickets', { method: 'POST' })
    if (ticket.protocol !== REALTIME_PROTOCOL) throw protocolError('unsupported_protocol', 'This server requires a different client version.')
    if (this.serverId && this.serverId !== ticket.server_id) throw protocolError('server_mismatch', 'The server instance changed. Sign in again.')
    this.serverId = ticket.server_id
    const url = new URL(`${this.origin}/api/v2/realtime`, window.location.href)
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(url, REALTIME_PROTOCOL)
    this.socket = socket
    socket.onmessage = message => {
      if (this.socket !== socket || this.controller.signal.aborted) return
      clearTimeout(this.heartbeatTimer)
      this.heartbeatTimer = setTimeout(() => socket.close(), 65_000)
      try {
        const frame = JSON.parse(String(message.data)) as RealtimeFrame
        if (frame.type === 'hello') {
          if (frame.server_id !== ticket.server_id || frame.protocol !== REALTIME_PROTOCOL) throw protocolError('unsupported_protocol')
          this.send({ type: 'authenticate', ticket: ticket.ticket })
        } else if (frame.type === 'authenticated') {
          clearTimeout(this.handshakeTimer)
          this.authenticated = true
          for (const listener of this.stateListeners) listener('connected')
          this.retry = 0
          for (const scope of this.listeners.keys()) this.subscribeScope(scope)
          for (const id of this.pending.keys()) void this.readReceipt(id)
          this.readyResolve?.()
        } else if (frame.type === 'ping') this.send({ type: 'pong' })
        else if (frame.type === 'command_result' && typeof frame.request_id === 'string') {
          const error = frame.error as { code: string; message?: string; details?: unknown } | undefined
          this.settle(frame.request_id, frame.result as Receipt, error ? new ApiError(409, error.code, error.message ?? error.code, error.details) : undefined)
        } else if ((frame.type === 'event' || frame.type === 'events') && frame.scope) {
          // Note: the server batches a poll's events into one frame so a long run
          // costs one task, one synchronous store pass, and one ack per batch.
          const events = frame.type === 'events' ? frame.events as RealtimeFrame[] : [frame]
          if (!Array.isArray(events)) throw protocolError('invalid_frame')
          const run = frame.scope.startsWith('run:')
          for (const event of events) if (!validCursor(event.cursor, run)) throw protocolError('invalid_cursor')
          for (const listener of [...(this.listeners.get(frame.scope) ?? [])]) {
            // Filter per listener before merging so a merged delta never repeats
            // text a listener with a later cursor already applied.
            const fresh = events.filter(event => BigInt(event.cursor!) > BigInt(listener.cursor))
            for (const event of coalesceTextEvents(fresh)) {
              listener.apply(event)
              listener.cursor = event.cursor!
            }
          }
          const last = events.at(-1)?.cursor
          if (last !== undefined) this.send({ type: 'ack', scope: frame.scope, cursor: last })
        } else if (frame.type === 'resync_required' || (frame.type === 'error' && frame.scope)) {
          for (const listener of [...(this.listeners.get(frame.scope ?? '') ?? [])]) listener.resync(protocolError(String(frame.code ?? 'resync_required'), String(frame.message ?? 'State needs to be refreshed.')))
        } else if (frame.type === 'error') throw protocolError(String(frame.code))
      } catch (error) {
        if (error instanceof ApiError && ['unsupported_protocol', 'server_mismatch', 'unauthorized'].includes(error.code)) this.fatal = error
        socket.close()
        this.disconnected(error)
      }
    }
    socket.onerror = () => socket.close()
    socket.onclose = () => { if (this.socket === socket) this.disconnected(new TypeError('Realtime connection closed')) }
  }

  private disconnected(error: unknown) {
    if (error instanceof ApiError && ([401, 403, 404].includes(error.status) || ['unsupported_protocol', 'server_mismatch'].includes(error.code))) this.fatal = error
    clearTimeout(this.handshakeTimer)
    clearTimeout(this.heartbeatTimer)
    const socket = this.socket
    this.socket = undefined
    socket?.close()
    this.authenticated = false
    for (const listener of this.stateListeners) listener(this.fatal ? 'error' : 'connecting')
    this.readyReject?.(error as Error)
    this.ready = undefined
    this.readyReject = undefined
    this.readyResolve = undefined
    if (this.controller.signal.aborted) return
    if (this.fatal) {
      for (const set of this.listeners.values()) for (const listener of [...set]) listener.resync(this.fatal)
      for (const id of this.pending.keys()) this.settle(id, undefined, this.fatal)
    } else {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = setTimeout(() => { void this.connect().catch(() => undefined) }, retryDelayMs(++this.retry))
    }
  }

  private reconnect = () => {
    if (this.authenticated) this.socket?.close()
    void this.connect().catch(() => undefined)
  }
  private foreground = () => { if (!document.hidden) this.reconnect() }

  dispose() {
    this.cleanupAuth()
    window.removeEventListener('online', this.reconnect)
    window.removeEventListener('qunica:reconnect', this.reconnect)
    document.removeEventListener('visibilitychange', this.foreground)
    clearTimeout(this.reconnectTimer)
    clearTimeout(this.handshakeTimer)
    clearTimeout(this.heartbeatTimer)
    const error = new DOMException('Session changed', 'AbortError')
    this.readyReject?.(error)
    this.socket?.close()
    this.socket = undefined
    this.listeners.clear()
    this.stateListeners.clear()
    this.localRuns.clear()
    this.localContinuationThreads.clear()
    for (const id of this.pending.keys()) this.settle(id, undefined, error)
    if (!this.controller.signal.aborted) this.controller.abort()
  }
}

let shared: RealtimeClient | undefined
export function realtimeClient(token: string): RealtimeClient {
  const origin = apiOrigin()
  if (!shared || shared.controller.signal.aborted || shared.token !== token || shared.origin !== origin) {
    shared?.dispose()
    shared = new RealtimeClient(origin, token)
  }
  return shared
}
