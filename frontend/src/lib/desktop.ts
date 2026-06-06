/**
 * Desktop-only (Tauri) bridges for OS file operations.
 *
 * All helpers dynamically import the Tauri API so the module stays inert in the
 * browser build. Guard calls with `isDesktopRuntime()` from `@/lib/runtime`.
 */

import { isDesktopRuntime } from '@/lib/runtime'

export { isDesktopRuntime }

/** Reveal (and select) an absolute path in the OS file manager. */
export async function revealInFileManager(absPath: string): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('reveal_in_file_manager', { path: absPath })
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunkSize = 0x8000
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize))
  }
  return btoa(binary)
}

/**
 * Show a native "Save As" dialog and write the bytes to the chosen path.
 * Returns the saved absolute path, or null if the user cancelled.
 */
export async function saveFileViaDialog(
  name: string,
  data: Uint8Array,
): Promise<string | null> {
  const { invoke } = await import('@tauri-apps/api/core')
  const result = await invoke<string | null>('save_file', {
    name,
    contents_b64: bytesToBase64(data),
  })
  return result ?? null
}
