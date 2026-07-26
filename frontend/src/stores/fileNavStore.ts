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
  /**
   * Which root the path belongs to: `null` is the conversation workspace, a
   * string is that agent's own folder. A path means nothing without its root.
   */
  agentId: string | null
  nonce: number
}

interface FileNavState {
  request: FileNavRequest | null
  openFile: (groupId: string, path: string, agentId?: string | null) => void
  clear: () => void
}

export const useFileNavStore = create<FileNavState>((set) => ({
  request: null,
  openFile: (groupId, path, agentId = null) =>
    set((s) => ({
      request: { groupId, path, agentId, nonce: (s.request?.nonce ?? 0) + 1 },
    })),
  clear: () => set({ request: null }),
}))
