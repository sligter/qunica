import { create } from 'zustand'

/**
 * Cross-component bus for "open this workspace file in the right-hand panel".
 * Chat file links publish a request; the workspace panel (and the page that
 * controls its visibility) react to it. `nonce` makes repeated clicks on the
 * same path re-trigger the effect.
 */
export interface FileNavRequest {
  groupId: string
  path: string
  nonce: number
}

interface FileNavState {
  request: FileNavRequest | null
  openFile: (groupId: string, path: string) => void
  clear: () => void
}

export const useFileNavStore = create<FileNavState>((set) => ({
  request: null,
  openFile: (groupId, path) =>
    set((s) => ({
      request: { groupId, path, nonce: (s.request?.nonce ?? 0) + 1 },
    })),
  clear: () => set({ request: null }),
}))
