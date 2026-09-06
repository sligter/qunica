import { invoke } from '@tauri-apps/api/core'

const CHUNK_SIZE = 64 * 1024

/** Transfer bounded chunks, then let Android's document picker choose the destination. */
export async function saveAndroidFile(name: string, blob: Blob): Promise<string | null> {
  const { id } = await invoke<{ id: string }>('mobile_file_export', {
    operation: 'begin', payload: { name, mime: blob.type || 'application/octet-stream', size: blob.size },
  })
  try {
    for (let offset = 0; offset < blob.size; offset += CHUNK_SIZE) {
      const bytes = new Uint8Array(await blob.slice(offset, offset + CHUNK_SIZE).arrayBuffer())
      let binary = ''
      for (let start = 0; start < bytes.length; start += 8192) {
        binary += String.fromCharCode(...bytes.subarray(start, start + 8192))
      }
      await invoke('mobile_file_export', {
        operation: 'append', payload: { id, offset, data: btoa(binary) },
      })
    }
    const { uri } = await invoke<{ uri: string | null }>('mobile_file_export', {
      operation: 'save', payload: { id },
    })
    return uri
  } finally {
    // Also clean up partial staging on bridge failure; never hide the original error.
    await invoke('mobile_file_export', { operation: 'discard', payload: { id } }).catch(() => undefined)
  }
}
