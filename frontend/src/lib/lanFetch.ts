import { invoke } from '@tauri-apps/api/core'
import { LAN_ORIGIN } from './androidSession'

/** Transport loss does not prove a mutation was rejected by the desktop. */
export class LanTransportError extends TypeError {
  readonly requestMayHaveBeenSent = true
}

function encode(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192))
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '')
}
function decode(value: string): Uint8Array {
  return Uint8Array.from(atob(value.replaceAll('-', '+').replaceAll('_', '/')), c => c.charCodeAt(0))
}

/** Native encrypted TCP, presented as a streaming Fetch response to existing API/SSE code. */
export async function lanFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const request = new Request(input, init)
  const url = new URL(request.url)
  if (url.origin !== LAN_ORIGIN || !url.pathname.startsWith('/api/v2/')) throw new Error('Only paired desktop API requests are allowed')
  request.signal.throwIfAborted()
  const body = new Uint8Array(await request.arrayBuffer())
  if (body.length > 32 * 1024 * 1024) throw new Error('Upload exceeds 32 MiB')
  const id = await invoke<string>('mobile_lan_prepare')
  let closed = false
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined
  const close = () => {
    if (closed) return
    closed = true
    request.signal.removeEventListener('abort', abort)
    void invoke('mobile_lan_close', { id }).catch(() => undefined)
  }
  const abort = () => {
    if (closed) return
    controller?.error(request.signal.reason ?? new DOMException('Aborted', 'AbortError'))
    close()
  }
  request.signal.addEventListener('abort', abort, { once: true })
  try {
    if (request.signal.aborted) { abort(); request.signal.throwIfAborted() }
    const head = await invoke<{ status: number; headers: [string, string][] }>('mobile_lan_open', {
      id,
      head: { method: request.method, path: url.pathname + url.search, headers: Array.from(request.headers.entries()) },
      body: encode(body),
    })
    request.signal.throwIfAborted()
    if ([204, 205, 304].includes(head.status) || request.method === 'HEAD') {
      close()
      return new Response(null, head)
    }
    const stream = new ReadableStream<Uint8Array>({
      start(value) { controller = value },
      async pull(value) {
        if (closed) return
        try {
          const chunk = await invoke<{ end: boolean; data: string }>('mobile_lan_read', { id })
          if (closed) return
          if (chunk.end) { value.close(); close() }
          else value.enqueue(decode(chunk.data))
        } catch (error) {
          if (!closed) { value.error(new LanTransportError(String(error))); close() }
        }
      },
      cancel: close,
    })
    return new Response(stream, head)
  } catch (error) { close(); request.signal.throwIfAborted(); throw new LanTransportError(String(error)) }
}
