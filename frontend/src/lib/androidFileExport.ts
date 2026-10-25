import { invoke } from '@tauri-apps/api/core'

const CHUNK_SIZE = 64 * 1024

async function exportChunks(
  name: string, mime: string, size: number, chunks: AsyncIterable<Uint8Array>,
  onProgress?: (received: number) => void, signal?: AbortSignal,
): Promise<string | null> {
  signal?.throwIfAborted()
  const { id } = await invoke<{ id: string }>('mobile_file_export', {
    operation: 'begin', payload: { name, mime: mime || 'application/octet-stream', size },
  })
  try {
    let offset = 0
    for await (const chunk of chunks) {
      if (chunk.length > size - offset) throw new Error('Download size changed. Refresh and try again.')
      for (let start = 0; start < chunk.length; start += CHUNK_SIZE) {
        signal?.throwIfAborted()
        const bytes = chunk.subarray(start, start + CHUNK_SIZE)
        let binary = ''
        for (let i = 0; i < bytes.length; i += 8192) {
          binary += String.fromCharCode(...bytes.subarray(i, i + 8192))
        }
        await invoke('mobile_file_export', {
          operation: 'append', payload: { id, offset, data: btoa(binary) },
        })
        offset += bytes.length
        onProgress?.(offset)
      }
    }
    signal?.throwIfAborted()
    if (offset !== size) throw new Error('Download is incomplete. Try again.')
    onProgress?.(offset)
    const { uri } = await invoke<{ uri: string | null }>('mobile_file_export', {
      operation: 'save', payload: { id },
    })
    return uri
  } finally {
    // Also clean up partial staging on bridge failure; never hide the original error.
    await invoke('mobile_file_export', { operation: 'discard', payload: { id } }).catch(() => undefined)
  }
}

/** Transfer bounded chunks, then let Android's document picker choose the destination. */
export async function saveAndroidFile(name: string, blob: Blob): Promise<string | null> {
  async function* chunks() {
    for (let offset = 0; offset < blob.size; offset += CHUNK_SIZE) {
      yield new Uint8Array(await blob.slice(offset, offset + CHUNK_SIZE).arrayBuffer())
    }
  }
  return exportChunks(name, blob.type, blob.size, chunks())
}

/** Note: artifacts reach 1000 MiB; stream into native staging without a whole-file Blob. */
export async function saveAndroidResponse(
  name: string, response: Response, size: number, onProgress?: (received: number) => void,
  signal?: AbortSignal,
): Promise<string | null> {
  const reader = response.body?.getReader()
  const abort = () => { void reader?.cancel().catch(() => undefined) }
  signal?.addEventListener('abort', abort, { once: true })
  async function* chunks() {
    while (reader) {
      signal?.throwIfAborted()
      const { done, value } = await reader.read()
      if (done) break
      yield value
    }
  }
  try {
    if (!response.ok || !Number.isSafeInteger(size) || size < 0 || size > 1000 * 1024 * 1024) {
      throw new Error('Invalid download response or size.')
    }
    return await exportChunks(name, response.headers.get('content-type') ?? '', size, chunks(), onProgress, signal)
  } finally {
    signal?.removeEventListener('abort', abort)
    await reader?.cancel().catch(() => undefined)
    reader?.releaseLock()
  }
}
