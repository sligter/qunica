/**
 * Browser folder picker shim.
 *
 * - Prefers the modern `window.showDirectoryPicker()` (Chromium-based) which
 *   returns a handle WITHOUT exposing the absolute filesystem path; we only
 *   read `handle.name`.
 * - When the modern API is unavailable, callers should use the legacy
 *   `<input type="file" webkitdirectory>` element themselves; this module
 *   reports `kind: 'fallback'` so the caller can trigger that input.
 *
 * No file content is uploaded or read. Only the picked folder name is
 * surfaced — backends still need the absolute path entered manually.
 */

interface FileSystemDirectoryHandleLike {
  readonly name: string
}

interface ShowDirectoryPickerWindow extends Window {
  showDirectoryPicker?: () => Promise<FileSystemDirectoryHandleLike>
}

export type FolderPickResult =
  | { kind: 'native'; name: string }
  | { kind: 'cancelled' }
  | { kind: 'fallback' }

export async function pickFolder(): Promise<FolderPickResult> {
  const showDirectoryPicker = (window as ShowDirectoryPickerWindow)
    .showDirectoryPicker
  if (typeof showDirectoryPicker !== 'function') {
    return { kind: 'fallback' }
  }
  try {
    const handle = await showDirectoryPicker()
    return { kind: 'native', name: handle.name }
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') {
      return { kind: 'cancelled' }
    }
    return { kind: 'fallback' }
  }
}
