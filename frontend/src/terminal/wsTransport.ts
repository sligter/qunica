import { apiOrigin } from '@/lib/runtime'
import { abortOnAuthChange } from '@/lib/authFetch'
import { retryDelayMs } from '@/lib/api-v2/retry'
import { ApiError } from '@/lib/api-v2/client'
import { realtimeClient } from '@/lib/realtime/client'
import { useAuthStore } from '@/stores/authStore'
import { terminalRequest, wireEventToTerminalEvent, type WireDescriptor, type WireTerminalEvent } from './httpTransport'
import { toWellFormedUnicode, utf8InputChunks } from './encoding'
import { TerminalTransportError, type TerminalTransport } from './transport'
import type { TerminalEvent } from './types'

const PROTOCOL = 'qunica.terminal.v1'
class TerminalSocket {
  private socket?: WebSocket
  private generation: string | null = null
  private cursor = 0n
  private hasConnected = false
  private inputReleased = false
  private epoch = 0
  private retry = 0
  private timer?: ReturnType<typeof setTimeout>
  private handshake?: ReturnType<typeof setTimeout>
  private renewal?: ReturnType<typeof setInterval>
  private heartbeat?: ReturnType<typeof setTimeout>
  private controller = new AbortController()
  private pending = new Map<string, { resolve: () => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>()
  private cleanupAuth: () => void
  private readyResolve?: () => void
  private readyReject?: (error: Error) => void
  readonly ready = new Promise<void>((resolve, reject) => { this.readyResolve = resolve; this.readyReject = reject })
  get connectionEpoch() { return this.epoch }
  get writable() { return this.generation !== null }
  constructor(readonly id: string, readonly token: string, readonly onEvent: (event: TerminalEvent) => void, readonly repaint = false) {
    this.cleanupAuth = abortOnAuthChange(this.controller, token)
    this.controller.signal.addEventListener('abort', () => this.stop(), { once: true })
    document.addEventListener('visibilitychange', this.foreground)
    window.addEventListener('online', this.reconnect)
    void this.connect()
  }

  private async connect() {
    const epoch = ++this.epoch
    try {
      const ticket = await realtimeClient(this.token).request<{ ticket: string; server_id: string }>('/realtime/tickets', { method: 'POST' })
      if (this.controller.signal.aborted || epoch !== this.epoch) return
      const url = new URL(`${apiOrigin()}/api/v2/terminal/realtime`, window.location.href)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      const socket = new WebSocket(url, PROTOCOL)
      this.socket = socket
      this.handshake = setTimeout(() => socket.close(), 10_000)
      socket.onmessage = message => {
        if (this.controller.signal.aborted || epoch !== this.epoch) return
        clearTimeout(this.heartbeat)
        this.heartbeat = setTimeout(() => socket.close(), 65_000)
        try {
          const frame = JSON.parse(String(message.data)) as Record<string, unknown>
          if (frame.type === 'hello') {
            if (frame.protocol !== PROTOCOL || frame.server_id !== ticket.server_id) throw new TerminalTransportError('unsupported_protocol', 'Upgrade the client to connect to this terminal.')
            this.send({ type: 'authenticate', ticket: ticket.ticket, session_id: this.id, cursor: this.cursor.toString(), repaint: this.repaint && !this.hasConnected, request_input: !this.inputReleased })
          } else if (frame.type === 'authenticated') {
            clearTimeout(this.handshake)
            this.retry = 0
            if (!this.hasConnected && typeof frame.cursor === 'string') this.cursor = BigInt(frame.cursor)
            this.hasConnected = true
            this.control(frame.generation)
            this.readyResolve?.()
            clearInterval(this.renewal)
            this.renewal = setInterval(() => {
              if (this.controller.signal.aborted || epoch !== this.epoch) return
              if (this.inputReleased) return
              try { this.send(this.generation ? { type: 'renew_input', generation: this.generation } : { type: 'acquire_input' }) }
              catch { socket.close() }
            }, 5_000)
          } else if (frame.type === 'lease') { this.control(frame.generation); if (typeof frame.request_id === 'string') this.settle(frame.request_id) }
          else if (frame.type === 'ping') this.send({ type: 'pong' })
          else if (frame.type === 'output') {
            const cursor = BigInt(String(frame.cursor))
            if (cursor <= this.cursor) return
            if (cursor !== this.cursor + 1n) throw new TerminalTransportError('terminal.output_gap', 'Terminal output exceeded the replay window. Reopen the terminal to redraw.')
            const event = frame.event as WireTerminalEvent
            this.onEvent(wireEventToTerminalEvent(event))
            this.cursor = cursor
            if (event.event === 'exit' || event.event === 'error') this.stop()
          } else if (frame.type === 'ack' && typeof frame.request_id === 'string') this.settle(frame.request_id)
          else if (frame.type === 'error') {
            const error = new TerminalTransportError(String(frame.code), String(frame.message ?? frame.code))
            if (typeof frame.request_id === 'string' && this.pending.has(frame.request_id)) this.settle(frame.request_id, error)
            else this.fail(error)
          }
        } catch (error) { this.fail(error as Error) }
      }
      socket.onerror = () => socket.close()
      socket.onclose = () => { if (epoch === this.epoch) this.disconnected() }
    } catch (error) {
      if (this.controller.signal.aborted || epoch !== this.epoch) return
      // Note: ticket requests can fail before a socket exists. Keep the same
      // PTY and output cursor through transient failures; input is never replayed.
      if (error instanceof ApiError && error.status < 500 && ![408, 429].includes(error.status)) this.fail(error)
      else this.disconnected()
    }
  }

  private control(value: unknown) {
    this.generation = typeof value === 'string' ? value : null
    this.onEvent({ event: 'control', data: { writable: this.generation !== null } })
  }

  private send(value: Record<string, unknown>) {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN || this.socket.bufferedAmount > 4 * 1024 * 1024) throw new TerminalTransportError('terminal.disconnected', 'Terminal disconnected. Input was not resent.')
    this.socket.send(JSON.stringify(value))
  }
  command(type: 'input' | 'resize' | 'close' | 'acquire_input' | 'release_input', data: Record<string, unknown>): Promise<void> {
    if (!this.generation && !['acquire_input', 'release_input'].includes(type)) return Promise.reject(new TerminalTransportError('terminal.lease_required', 'Another device controls this terminal. This view is read-only.'))
    if (this.pending.size >= 128) return Promise.reject(new TerminalTransportError('terminal.backpressure', 'Terminal input is busy.'))
    const requestId = crypto.randomUUID()
    return new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => this.settle(requestId, new TerminalTransportError('terminal.input_unconfirmed', 'Input delivery is uncertain. It was not resent.')), 10_000)
      this.pending.set(requestId, { resolve, reject, timer })
      try { this.send({ type, request_id: requestId, generation: this.generation, ...data }) }
      catch (error) { this.settle(requestId, error as Error) }
    })
  }
  async setInputControl(acquire: boolean) {
    this.inputReleased = !acquire
    await this.command(acquire ? 'acquire_input' : 'release_input', {})
  }
  private settle(id: string, error?: Error) {
    const pending = this.pending.get(id)
    if (!pending) return
    this.pending.delete(id)
    clearTimeout(pending.timer)
    if (error) pending.reject(error)
    else pending.resolve()
  }
  private disconnected() {
    ++this.epoch
    clearTimeout(this.handshake)
    clearTimeout(this.heartbeat)
    clearInterval(this.renewal)
    this.socket = undefined
    const error = new TerminalTransportError('terminal.disconnected', 'Terminal disconnected. Input was not resent.')
    for (const id of this.pending.keys()) this.settle(id, error)
    if (this.controller.signal.aborted) return
    this.control(null)
    clearTimeout(this.timer)
    this.timer = setTimeout(() => { void this.connect() }, retryDelayMs(++this.retry))
  }
  private reconnect = () => {
    if (this.controller.signal.aborted) return
    ++this.epoch
    this.socket?.close()
    this.disconnected()
    clearTimeout(this.timer)
    void this.connect()
  }
  private foreground = () => { if (!document.hidden) this.reconnect() }
  private fail(error: Error) {
    this.readyReject?.(error)
    this.onEvent({ event: 'error', data: { code: error instanceof TerminalTransportError ? error.code : 'terminal.connection_failed', message: error.message } })
    this.stop()
  }
  stop() {
    this.cleanupAuth()
    clearTimeout(this.timer)
    clearTimeout(this.handshake)
    clearTimeout(this.heartbeat)
    clearInterval(this.renewal)
    document.removeEventListener('visibilitychange', this.foreground)
    window.removeEventListener('online', this.reconnect)
    ++this.epoch
    this.socket?.close()
    this.socket = undefined
    this.generation = null
    const error = new TerminalTransportError('terminal.disconnected', 'Terminal disconnected. Input was not resent.')
    for (const id of this.pending.keys()) this.settle(id, error)
    this.readyReject?.(error)
    if (!this.controller.signal.aborted) this.controller.abort()
  }
}

export function createWsTerminalTransport(): TerminalTransport {
  const sessions = new Map<string, TerminalSocket>()
  const tails = new Map<string, Promise<void>>()
  function session(id: string) {
    const value = sessions.get(id)
    if (!value) throw new TerminalTransportError('terminal.session_missing', 'Terminal session no longer exists. Open a new terminal.')
    return value
  }
  return {
    canAttach: true,
    setInputControl: (id, acquire) => session(id).setInputControl(acquire),
    async create(request, onEvent) {
      const token = useAuthStore.getState().token
      if (!token) throw new TerminalTransportError('terminal.sign_in_required', 'Sign in again to open a terminal.')
      const descriptor = request.existingSessionId
        ? await terminalRequest<WireDescriptor>(`/terminal/sessions/${encodeURIComponent(request.existingSessionId)}`, { method: 'GET' })
        : await terminalRequest<WireDescriptor>('/terminal/sessions', { method: 'POST', body: {
        conversation_id: request.conversationId, cwd: request.cwd, cols: request.cols, rows: request.rows, shell: request.shell,
      } })
      if (!descriptor?.session_id) throw new TerminalTransportError('terminal.create_failed', 'The server did not return a terminal session.')
      if (useAuthStore.getState().token !== token) throw new TerminalTransportError('terminal.sign_in_required', 'The signed-in account changed.')
      const socket = new TerminalSocket(descriptor.session_id, token, onEvent, !!request.existingSessionId)
      sessions.set(descriptor.session_id, socket)
      await socket.ready
      return { sessionId: descriptor.session_id, shellName: descriptor.shell_name, cwd: descriptor.cwd }
    },
    async write(_conversation, id, data) {
      // No queued input survives a disconnect: each chunk requires the same live lease.
      const current = session(id)
      const epoch = current.connectionEpoch
      const operation = (tails.get(id) ?? Promise.resolve()).then(async () => {
        for (const chunk of utf8InputChunks(toWellFormedUnicode(data))) {
          if (current.connectionEpoch !== epoch) throw new TerminalTransportError('terminal.disconnected', 'Terminal reconnected. Queued input was not resent.')
          await current.command('input', { data: chunk })
        }
      })
      const tail = operation.catch(() => undefined)
      tails.set(id, tail)
      void tail.finally(() => { if (tails.get(id) === tail) tails.delete(id) })
      return operation
    },
    resize: async (_conversation, id, cols, rows) => session(id).command('resize', { cols, rows }),
    async close(_conversation, id) {
      const current = sessions.get(id)
      if (!current) return
      try { if (current.writable) await current.command('close', {}) }
      finally { current.stop(); sessions.delete(id); tails.delete(id) }
    },
    async closeAll() {
      for (const [id, current] of sessions) {
        try { await current.command('close', {}) } catch { /* A disconnected/read-only view cannot close another device's PTY. */ }
        current.stop(); sessions.delete(id)
      }
      tails.clear()
    },
  }
}
