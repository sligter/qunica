import { create } from 'zustand'

import type { ConversationWorkspaceFileRead } from '@/types/api'

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

export interface WorkspaceEditorTab {
  id: string
  file: ConversationWorkspaceFileRead
  agentId: string | null
  dirty: boolean
}

interface WorkspaceEditorStage {
  tabs: WorkspaceEditorTab[]
  activeTabId: string | null
}

interface FileNavState {
  request: FileNavRequest | null
  editorStages: Record<string, WorkspaceEditorStage>
  openFile: (groupId: string, path: string, agentId?: string | null) => void
  openEditor: (
    groupId: string,
    file: ConversationWorkspaceFileRead,
    agentId?: string | null,
  ) => void
  showChat: (groupId: string) => void
  activateEditor: (groupId: string, tabId: string) => void
  closeEditor: (groupId: string, tabId: string) => void
  setEditorDirty: (groupId: string, tabId: string, dirty: boolean) => void
  renameEditorPath: (
    groupId: string,
    agentId: string | null,
    oldPath: string,
    newPath: string,
  ) => void
  closeEditorPaths: (
    groupId: string,
    agentId: string | null,
    files?: Pick<ConversationWorkspaceFileRead, 'path' | 'is_dir'>[],
  ) => void
  clear: () => void
}

let editorTabNonce = 0

function renamedPath(path: string, oldPath: string, newPath: string): string | null {
  if (path === oldPath) return newPath
  if (path.startsWith(`${oldPath}/`)) return `${newPath}${path.slice(oldPath.length)}`
  return null
}

export const useFileNavStore = create<FileNavState>((set) => ({
  request: null,
  editorStages: {},
  openFile: (groupId, path, agentId = null) =>
    set((s) => ({
      request: { groupId, path, agentId, nonce: (s.request?.nonce ?? 0) + 1 },
    })),
  openEditor: (groupId, file, agentId = null) =>
    set((state) => {
      const stage = state.editorStages[groupId] ?? { tabs: [], activeTabId: null }
      const existing = stage.tabs.find(
        (tab) => tab.agentId === agentId && tab.file.path === file.path,
      )
      if (existing) {
        if (stage.activeTabId === existing.id) return state
        return {
          editorStages: {
            ...state.editorStages,
            [groupId]: { ...stage, activeTabId: existing.id },
          },
        }
      }
      const tab: WorkspaceEditorTab = {
        id: `workspace-editor-${++editorTabNonce}`,
        file,
        agentId,
        dirty: false,
      }
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: { tabs: [...stage.tabs, tab], activeTabId: tab.id },
        },
      }
    }),
  showChat: (groupId) =>
    set((state) => {
      const stage = state.editorStages[groupId]
      if (!stage || stage.activeTabId === null) return state
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: { ...stage, activeTabId: null },
        },
      }
    }),
  activateEditor: (groupId, tabId) =>
    set((state) => {
      const stage = state.editorStages[groupId]
      if (!stage || stage.activeTabId === tabId || !stage.tabs.some((tab) => tab.id === tabId)) {
        return state
      }
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: { ...stage, activeTabId: tabId },
        },
      }
    }),
  closeEditor: (groupId, tabId) =>
    set((state) => {
      const stage = state.editorStages[groupId]
      if (!stage) return state
      const closingIndex = stage.tabs.findIndex((tab) => tab.id === tabId)
      if (closingIndex < 0) return state
      const tabs = stage.tabs.filter((tab) => tab.id !== tabId)
      const activeTabId = stage.activeTabId === tabId
        ? (tabs[closingIndex]?.id ?? tabs[closingIndex - 1]?.id ?? null)
        : stage.activeTabId
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: { tabs, activeTabId },
        },
      }
    }),
  setEditorDirty: (groupId, tabId, dirty) =>
    set((state) => {
      const stage = state.editorStages[groupId]
      const tab = stage?.tabs.find((candidate) => candidate.id === tabId)
      if (!stage || !tab || tab.dirty === dirty) return state
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: {
            ...stage,
            tabs: stage.tabs.map((candidate) => (
              candidate.id === tabId ? { ...candidate, dirty } : candidate
            )),
          },
        },
      }
    }),
  renameEditorPath: (groupId, agentId, oldPath, newPath) =>
    set((state) => {
      const stage = state.editorStages[groupId]
      if (!stage) return state
      let changed = false
      const tabs = stage.tabs.map((tab) => {
        if (tab.agentId !== agentId) return tab
        const path = renamedPath(tab.file.path, oldPath, newPath)
        if (!path) return tab
        changed = true
        return {
          ...tab,
          file: { ...tab.file, path, name: path.split('/').at(-1) ?? path },
        }
      })
      if (!changed) return state
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: { ...stage, tabs },
        },
      }
    }),
  closeEditorPaths: (groupId, agentId, files) =>
    set((state) => {
      const stage = state.editorStages[groupId]
      if (!stage) return state
      const shouldClose = (tab: WorkspaceEditorTab) => tab.agentId === agentId && (
        !files || files.some((file) => (
          tab.file.path === file.path
          || (file.is_dir && tab.file.path.startsWith(`${file.path}/`))
        ))
      )
      const tabs = stage.tabs.filter((tab) => !shouldClose(tab))
      if (tabs.length === stage.tabs.length) return state
      const activeTabId = stage.activeTabId && tabs.some((tab) => tab.id === stage.activeTabId)
        ? stage.activeTabId
        : (tabs.at(-1)?.id ?? null)
      return {
        editorStages: {
          ...state.editorStages,
          [groupId]: { tabs, activeTabId },
        },
      }
    }),
  clear: () => set({ request: null }),
}))
