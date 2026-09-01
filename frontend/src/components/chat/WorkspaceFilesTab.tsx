import { useEffect, useId, useRef, useState } from 'react'
import {
  AppWindow,
  ChevronLeft,
  ChevronRight,
  ClipboardPaste,
  Copy,
  Download,
  Eraser,
  Eye,
  EyeOff,
  File,
  FilePlus,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  Pencil,
  PanelsTopLeft,
  RefreshCw,
  Scissors,
  Trash2,
  Upload,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { WorkspacePreviewRouter } from '@/components/chat/workspace-preview/WorkspacePreviewRouter'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { SearchInput } from '@/components/ui/search-input'
import {
  useConversationWorkspaceFiles,
  useConversationWorkspaceRoots,
  useDownloadConversationWorkspaceFile,
  useUploadConversationWorkspaceFile,
} from '@/hooks/useConversationWorkspaceFiles'
import {
  useCreateGroupWorkspaceEntry,
  useDeleteGroupWorkspaceFile,
  useRenameGroupWorkspaceFile,
  useWorkspaceFileActions,
  type WorkspaceEntryKind,
} from '@/hooks/useGroupFiles'
import { normalizeLanguage } from '@/i18n'
import {
  workspaceErrorMessageKey,
  type WorkspaceErrorMessageKey,
} from '@/i18n/localizedError'
import { isDesktopRuntime, revealInFileManager } from '@/lib/desktop'
import { formatNumber } from '@/lib/format'
import { cn } from '@/lib/utils'
import {
  encodeWorkspaceDragItems,
  WORKSPACE_ITEM_MIME,
  type WorkspaceDragItemInput,
} from '@/lib/workspaceDrag'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { ConversationScope, ConversationWorkspaceFileRead } from '@/types/api'

interface WorkspaceFilesTabProps {
  scope: ConversationScope
  conversationId: string | undefined
  workspaceId: string | null
}

type WorkspacePreviewMode = 'dialog' | 'editor'

const WORKSPACE_PREVIEW_MODE_KEY_PREFIX = 'qunica:conversations:workspace-preview-mode:'
const WORKSPACE_SHOW_HIDDEN_KEY_PREFIX = 'qunica:conversations:workspace-show-hidden:'
const FILE_ROW_HEIGHT = 32
const FILE_LIST_OVERSCAN = 8
const FILE_LIST_FALLBACK_HEIGHT = 320
// How far a pointer may travel between press and release and still count as a
// click on the row. Anything further was an attempt to drag it.
const FILE_ROW_CLICK_SLOP = 4

function previewModeStorageKey(scope: ConversationScope, conversationId: string): string {
  return `${WORKSPACE_PREVIEW_MODE_KEY_PREFIX}${scope}:${conversationId}`
}

function readPreviewMode(
  scope: ConversationScope,
  conversationId: string | undefined,
): WorkspacePreviewMode {
  if (!conversationId) return 'dialog'
  return localStorage.getItem(previewModeStorageKey(scope, conversationId)) === 'editor'
    ? 'editor'
    : 'dialog'
}

function showHiddenStorageKey(scope: ConversationScope, conversationId: string): string {
  return `${WORKSPACE_SHOW_HIDDEN_KEY_PREFIX}${scope}:${conversationId}`
}

function readShowHidden(scope: ConversationScope, conversationId: string | undefined): boolean {
  return Boolean(
    conversationId
    && localStorage.getItem(showHiddenStorageKey(scope, conversationId)) === 'true',
  )
}

function parentPath(path: string): string {
  const parts = path.split('/').filter(Boolean)
  parts.pop()
  return parts.join('/')
}

function fileName(path: string): string {
  return path.replaceAll('\\', '/').split('/').at(-1) || path
}

function formatSize(size: number | null | undefined, language: 'en-US' | 'zh-CN'): string {
  if (size == null) return ''
  if (size < 1024) return `${formatNumber(size, language)} B`
  if (size < 1024 * 1024) {
    return `${formatNumber(Number((size / 1024).toFixed(1)), language)} KB`
  }
  return `${formatNumber(Number((size / (1024 * 1024)).toFixed(1)), language)} MB`
}

function dragItem(file: ConversationWorkspaceFileRead): WorkspaceDragItemInput {
  return {
    path: file.path,
    name: file.name,
    kind: file.is_dir ? 'directory' : 'file',
  }
}

function encodeDragPayload(files: ConversationWorkspaceFileRead[]): string | null {
  try {
    return encodeWorkspaceDragItems(files.map(dragItem))
  } catch {
    // The encoder rejects the whole batch over one unrepresentable path, and
    // throwing out of `dragstart` would leave the drag with no payload at all.
    // The `text/plain` path list still drops as a usable fallback.
    return null
  }
}

export function WorkspaceFilesTab({
  scope,
  conversationId,
  workspaceId,
}: WorkspaceFilesTabProps) {
  const { t, i18n } = useTranslation(['chat', 'common'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const [currentPath, setCurrentPath] = useState('')
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null)
  const [previewFile, setPreviewFile] = useState<ConversationWorkspaceFileRead | null>(null)
  const [selectedWorkspacePaths, setSelectedWorkspacePaths] = useState<Set<string>>(
    () => new Set(),
  )
  const [isPreviewOpen, setIsPreviewOpen] = useState(false)
  const [previewMode, setPreviewMode] = useState<WorkspacePreviewMode>(
    () => readPreviewMode(scope, conversationId),
  )
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [showHidden, setShowHidden] = useState(
    () => readShowHidden(scope, conversationId),
  )
  const [searchInput, setSearchInput] = useState('')
  const [search, setSearch] = useState('')
  const [fileListFirstRow, setFileListFirstRow] = useState(0)
  const [fileListHeight, setFileListHeight] = useState(FILE_LIST_FALLBACK_HEIGHT)
  const [renaming, setRenaming] = useState<ConversationWorkspaceFileRead | null>(null)
  const [creating, setCreating] = useState<{ kind: WorkspaceEntryKind; parent: string } | null>(
    null,
  )
  const [createName, setCreateName] = useState('')
  const [pendingDelete, setPendingDelete] = useState<ConversationWorkspaceFileRead[] | null>(null)
  const [pendingClear, setPendingClear] = useState(false)
  const [movePaths, setMovePaths] = useState<string[] | null>(null)
  const [moveDestination, setMoveDestination] = useState('')
  const [clipboard, setClipboard] = useState<{
    mode: 'copy' | 'move'
    paths: string[]
  } | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [operationError, setOperationError] = useState<WorkspaceErrorMessageKey | null>(null)
  const [downloadingPath, setDownloadingPath] = useState<string | null>(null)
  const [draggingPath, setDraggingPath] = useState<string | null>(null)
  const [menu, setMenu] = useState<{
    x: number
    y: number
    file: ConversationWorkspaceFileRead | null
  } | null>(null)
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const fileListRef = useRef<HTMLDivElement | null>(null)
  const fileButtonRefs = useRef(new Map<string, HTMLButtonElement>())
  const menuFirstItemRef = useRef<HTMLButtonElement | null>(null)
  const selectionAnchorRef = useRef<string | null>(null)
  const didDragRef = useRef(false)
  const pointerOriginRef = useRef<{ x: number; y: number } | null>(null)
  const dragDescriptionId = useId()
  const contextMenuId = useId()
  const activeConversationId = workspaceId ? conversationId : undefined
  const hasConversation = Boolean(activeConversationId)
  const canUpload = hasConversation
  const canMutate = hasConversation
  const canRevealInFileManager = isDesktopRuntime()
  const roots = useConversationWorkspaceRoots(scope, activeConversationId)
  const rootEntries = roots.data ?? []
  // A root can disappear (agent removed, mode changed); fall back to the
  // conversation rather than leaving the panel pointed at nothing.
  const activeAgentId =
    selectedAgentId && rootEntries.some((entry) => entry.agent_id === selectedAgentId)
      ? selectedAgentId
      : null
  const activeRoot = rootEntries.find((entry) => entry.agent_id === activeAgentId) ?? null
  const files = useConversationWorkspaceFiles(
    scope,
    activeConversationId,
    search ? '' : currentPath,
    activeAgentId,
    showHidden,
    search,
  )
  const upload = useUploadConversationWorkspaceFile(scope, activeConversationId, activeAgentId)
  const download = useDownloadConversationWorkspaceFile(scope, activeConversationId, activeAgentId)
  const rename = useRenameGroupWorkspaceFile(activeConversationId, scope, activeAgentId)
  const create = useCreateGroupWorkspaceEntry(activeConversationId, scope, activeAgentId)
  const del = useDeleteGroupWorkspaceFile(activeConversationId, scope, activeAgentId)
  const actions = useWorkspaceFileActions(activeConversationId, scope, activeAgentId)
  const navRequest = useFileNavStore((state) => state.request)
  const clearNav = useFileNavStore((state) => state.clear)
  const openEditor = useFileNavStore((state) => state.openEditor)
  const renameEditorPath = useFileNavStore((state) => state.renameEditorPath)
  const closeEditorPaths = useFileNavStore((state) => state.closeEditorPaths)

  const title = currentPath
    || activeRoot?.display_name
    || activeRoot?.name
    || t('chat:workspace.root')
  const sortedFiles = files.data ?? []
  const isSearchMode = Boolean(searchInput.trim())
  const isSearchPending = searchInput.trim() !== search
  const visibleRowCount = Math.ceil(fileListHeight / FILE_ROW_HEIGHT)
  const effectiveFirstRow = Math.min(
    fileListFirstRow,
    Math.max(0, sortedFiles.length - visibleRowCount),
  )
  const visibleStart = Math.max(0, effectiveFirstRow - FILE_LIST_OVERSCAN)
  const visibleEnd = Math.min(
    sortedFiles.length,
    effectiveFirstRow + visibleRowCount + FILE_LIST_OVERSCAN,
  )
  const visibleFiles = isSearchPending ? [] : sortedFiles.slice(visibleStart, visibleEnd)
  const selectedCount = selectedWorkspacePaths.size
  const selectedFiles = sortedFiles.filter((file) => selectedWorkspacePaths.has(file.path))

  const changePreviewMode = (mode: WorkspacePreviewMode) => {
    setPreviewMode(mode)
    if (conversationId) {
      localStorage.setItem(previewModeStorageKey(scope, conversationId), mode)
    }
  }

  const changeSearchInput = (value: string) => {
    setSearchInput(value)
    if (!value.trim()) setSearch('')
  }

  const toggleShowHidden = () => {
    const next = !showHidden
    setShowHidden(next)
    setSelectedWorkspacePaths(new Set())
    if (!next && currentPath.split('/').some((part) => part.startsWith('.'))) {
      setCurrentPath('')
    }
    if (conversationId) {
      localStorage.setItem(showHiddenStorageKey(scope, conversationId), String(next))
    }
  }

  const selectOnlyPath = (path: string) => {
    selectionAnchorRef.current = path
    setSelectedWorkspacePaths(new Set([path]))
  }

  const toggleSelectedPath = (path: string) => {
    selectionAnchorRef.current = path
    setSelectedWorkspacePaths((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const selectPathRange = (path: string, additive: boolean) => {
    const anchor = selectionAnchorRef.current
    const anchorIndex = sortedFiles.findIndex((file) => file.path === anchor)
    const targetIndex = sortedFiles.findIndex((file) => file.path === path)
    if (anchorIndex < 0 || targetIndex < 0) {
      selectOnlyPath(path)
      return
    }
    const start = Math.min(anchorIndex, targetIndex)
    const end = Math.max(anchorIndex, targetIndex)
    setSelectedWorkspacePaths((current) => {
      const next = additive ? new Set(current) : new Set<string>()
      for (const file of sortedFiles.slice(start, end + 1)) next.add(file.path)
      return next
    })
  }

  const selectedDragFiles = (file: ConversationWorkspaceFileRead) => {
    if (!selectedWorkspacePaths.has(file.path)) return [file]
    const selected = sortedFiles.filter((candidate) => selectedWorkspacePaths.has(candidate.path))
    return selected.length > 0 ? selected : [file]
  }

  const openFile = (
    file: ConversationWorkspaceFileRead,
    mode: WorkspacePreviewMode = previewMode,
  ) => {
    selectOnlyPath(file.path)
    if (mode === 'editor' && activeConversationId) {
      openEditor(activeConversationId, file, activeAgentId)
    } else {
      setPreviewFile(file)
      setIsPreviewOpen(true)
    }
  }

  const openEntry = (file: ConversationWorkspaceFileRead) => {
    if (file.is_dir) {
      selectOnlyPath(file.path)
      setSearchInput('')
      setSearch('')
      setCurrentPath(file.path)
    } else {
      openFile(file)
    }
  }

  const refreshFiles = async () => {
    setIsRefreshing(true)
    try {
      await files.refetch()
    } finally {
      setIsRefreshing(false)
    }
  }

  const handleFileClick = (
    event: React.MouseEvent<HTMLButtonElement>,
    file: ConversationWorkspaceFileRead,
  ) => {
    const origin = pointerOriginRef.current
    const draggedGesture = didDragRef.current
    didDragRef.current = false
    pointerOriginRef.current = null
    // A pointer that travelled before releasing meant to drag the row, not open
    // it. Deciding on distance rather than on `dragstart` matters because the
    // webview often never starts a native drag for a short or hurried gesture --
    // it delivers a plain click instead, and opening the preview there is what
    // makes dragging feel like it needs a second try. `detail === 0` marks a
    // keyboard-activated click, which has no meaningful coordinates.
    const travelled = event.detail > 0
      && origin !== null
      && (Math.abs(event.clientX - origin.x) > FILE_ROW_CLICK_SLOP
        || Math.abs(event.clientY - origin.y) > FILE_ROW_CLICK_SLOP)
    if (draggedGesture || travelled) return
    if (event.shiftKey) {
      selectPathRange(file.path, event.ctrlKey || event.metaKey)
      return
    }
    if (event.ctrlKey || event.metaKey) {
      toggleSelectedPath(file.path)
      return
    }
    openEntry(file)
  }

  const openContextMenu = (
    x: number,
    y: number,
    file: ConversationWorkspaceFileRead | null,
  ) => {
    if (!file && !hasConversation) return
    setMenu({
      x: Math.max(8, Math.min(x, window.innerWidth - 192)),
      y: Math.max(8, Math.min(y, window.innerHeight - 360)),
      file,
    })
  }

  const handleFileKeyDown = (
    event: React.KeyboardEvent<HTMLButtonElement>,
    file: ConversationWorkspaceFileRead,
  ) => {
    didDragRef.current = false
    pointerOriginRef.current = null
    if (event.key === 'Enter') {
      event.preventDefault()
      openEntry(file)
      return
    }
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return
    event.preventDefault()
    const rect = event.currentTarget.getBoundingClientRect()
    openContextMenu(rect.left + 8, rect.bottom, file)
  }

  const handleFileDragStart = (
    event: React.DragEvent<HTMLButtonElement>,
    file: ConversationWorkspaceFileRead,
  ) => {
    didDragRef.current = true
    const draggedFiles = selectedDragFiles(file)
    // Fill the payload before touching state. Selecting the row and marking it
    // grabbed both re-render the element the webview is dragging, and doing
    // that while `dragstart` is still running can cancel the gesture outright.
    event.dataTransfer.effectAllowed = 'copy'
    const structured = encodeDragPayload(draggedFiles)
    if (structured) event.dataTransfer.setData(WORKSPACE_ITEM_MIME, structured)
    event.dataTransfer.setData('text/plain', draggedFiles.map((item) => item.path).join('\n'))
    if (!selectedWorkspacePaths.has(file.path)) selectOnlyPath(file.path)
    setDraggingPath(file.path)
  }

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const next = searchInput.trim()
      setSearch((current) => (current === next ? current : next))
    }, 300)
    return () => window.clearTimeout(timer)
  }, [searchInput])

  useEffect(() => {
    const node = fileListRef.current
    if (!node) return
    const measure = () => {
      const next = node.clientHeight || FILE_LIST_FALLBACK_HEIGHT
      setFileListHeight((current) => (current === next ? current : next))
    }
    measure()
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', measure)
      return () => window.removeEventListener('resize', measure)
    }
    const observer = new ResizeObserver(measure)
    observer.observe(node)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    setCurrentPath('')
    setPreviewFile(null)
    setIsPreviewOpen(false)
    setRenaming(null)
    setCreating(null)
    setCreateName('')
    setPendingDelete(null)
    setPendingClear(false)
    setMovePaths(null)
    setMoveDestination('')
    setClipboard(null)
    setSearchInput('')
    setSearch('')
    setOperationError(null)
    setMenu(null)
  }, [conversationId, scope, workspaceId])

  useEffect(() => {
    setCurrentPath('')
    setPreviewFile(null)
    setIsPreviewOpen(false)
    setClipboard(null)
    setMovePaths(null)
    setSearchInput('')
    setSearch('')
  }, [activeAgentId])

  useEffect(() => {
    if (!navRequest || navRequest.groupId !== conversationId || !workspaceId) return
    const requestedAgentId = navRequest.agentId ?? null
    if (requestedAgentId !== activeAgentId) {
      setSelectedAgentId(requestedAgentId)
      return
    }
    // An empty path means "show me this root", not "open this file".
    if (!navRequest.path) {
      setSearchInput('')
      setSearch('')
      setCurrentPath('')
      setPreviewFile(null)
      setIsPreviewOpen(false)
      clearNav()
      return
    }
    const visibleMatch = navRequest.agentId === activeAgentId
      && !navRequest.path.includes('/')
      ? files.data?.find((file) => !file.is_dir && file.name === navRequest.path)
      : undefined
    const requestedFile: ConversationWorkspaceFileRead = visibleMatch ?? {
      path: navRequest.path,
      name: fileName(navRequest.path),
      is_dir: false,
      size: null,
      modified_at: null,
    }
    setSearchInput('')
    setSearch('')
    setCurrentPath(parentPath(requestedFile.path))
    selectionAnchorRef.current = requestedFile.path
    setSelectedWorkspacePaths(new Set([requestedFile.path]))
    if (previewMode === 'editor') {
      openEditor(conversationId, requestedFile, activeAgentId)
    } else {
      setPreviewFile(requestedFile)
      setIsPreviewOpen(true)
    }
    clearNav()
  }, [activeAgentId, clearNav, conversationId, files.data, navRequest, openEditor, previewMode, workspaceId])

  useEffect(() => {
    setSelectedWorkspacePaths(new Set())
    selectionAnchorRef.current = null
    setFileListFirstRow(0)
    if (fileListRef.current) fileListRef.current.scrollTop = 0
  }, [currentPath, conversationId, scope, search])

  useEffect(() => {
    if (!menu) return
    menuFirstItemRef.current?.focus()
    const close = () => setMenu(null)
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      const trigger = menu.file ? fileButtonRefs.current.get(menu.file.path) : null
      setMenu(null)
      requestAnimationFrame(() => trigger?.focus())
    }
    window.addEventListener('click', close)
    window.addEventListener('resize', close)
    window.addEventListener('scroll', close, true)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('click', close)
      window.removeEventListener('resize', close)
      window.removeEventListener('scroll', close, true)
      window.removeEventListener('keydown', onKey)
    }
  }, [menu])

  const handleMenuKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!menu) return
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      const trigger = menu.file ? fileButtonRefs.current.get(menu.file.path) : null
      setMenu(null)
      requestAnimationFrame(() => trigger?.focus())
      return
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    const items = Array.from(
      event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    )
    if (items.length === 0) return
    event.preventDefault()
    const activeIndex = items.indexOf(document.activeElement as HTMLButtonElement)
    const nextIndex = event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? items.length - 1
        : event.key === 'ArrowUp'
          ? (activeIndex <= 0 ? items.length - 1 : activeIndex - 1)
          : (activeIndex + 1) % items.length
    items[nextIndex]?.focus()
  }

  const startCreate = (kind: WorkspaceEntryKind, parent: string) => {
    if (!canMutate) return
    setCreating({ kind, parent })
    setCreateName('')
    setOperationError(null)
    create.reset()
  }

  const submitCreate = async () => {
    if (!creating) return
    const name = createName.trim()
    if (!name) return
    const path = creating.parent ? `${creating.parent}/${name}` : name
    try {
      await create.mutateAsync({ path, kind: creating.kind })
    } catch {
      // The dialog renders create.error; keep it open so the name can be fixed.
      return
    }
    setCreating(null)
    setCreateName('')
    // Show the folder the entry landed in, which is where the refetched
    // listing will surface it.
    setSearchInput('')
    setSearch('')
    setCurrentPath(creating.parent)
  }

  const startRename = (file: ConversationWorkspaceFileRead) => {
    if (!canMutate) return
    setRenaming(file)
    setRenameValue(file.path)
  }

  const submitRename = () => {
    if (!activeConversationId || !renaming || !renameValue.trim()) return
    const oldPath = renaming.path
    const wasPreviewOpen = isPreviewOpen && previewFile !== null
    void rename
      .mutateAsync({ path: oldPath, newPath: renameValue.trim() })
      .then((next) => {
        setRenaming(null)
        renameEditorPath(activeConversationId, activeAgentId, oldPath, next.path)
        setPreviewFile((selected) => {
          if (!selected) return selected
          if (selected.path === oldPath) return next
          if (selected.path.startsWith(`${oldPath}/`)) {
            const path = `${next.path}${selected.path.slice(oldPath.length)}`
            return { ...selected, path, name: fileName(path) }
          }
          return selected
        })
        setSelectedWorkspacePaths((current) => {
          const updated = new Set<string>()
          for (const path of current) {
            if (path === oldPath) updated.add(next.path)
            else if (path.startsWith(`${oldPath}/`)) {
              updated.add(`${next.path}${path.slice(oldPath.length)}`)
            } else updated.add(path)
          }
          return updated
        })
        setIsPreviewOpen(wasPreviewOpen)
        if (parentPath(oldPath) !== parentPath(next.path)) {
          setCurrentPath(parentPath(next.path))
        }
      })
      .catch(() => undefined)
  }

  const uploadFile = (file: globalThis.File | undefined) => {
    if (!file || !activeConversationId) return
    setOperationError(null)
    void upload
      .mutateAsync(file)
      .then(() => {
        setSearchInput('')
        setSearch('')
        setCurrentPath('uploads')
      })
      .catch(() => undefined)
      .finally(() => {
        if (fileInputRef.current) fileInputRef.current.value = ''
      })
  }

  const downloadFile = (file: ConversationWorkspaceFileRead) => {
    if (!activeConversationId || file.is_dir) return
    setOperationError(null)
    setDownloadingPath(file.path)
    void download
      .mutateAsync(file.path)
      .catch((error: unknown) => setOperationError(workspaceErrorMessageKey(error)))
      .finally(() => setDownloadingPath(null))
  }

  const revealFile = (file: ConversationWorkspaceFileRead) => {
    if (!canRevealInFileManager || !file.abs_path) return
    setOperationError(null)
    void revealInFileManager(file.abs_path)
      .catch((error: unknown) => setOperationError(workspaceErrorMessageKey(error)))
  }

  const filesForAction = (file: ConversationWorkspaceFileRead) => {
    if (!selectedWorkspacePaths.has(file.path)) return [file]
    return selectedFiles.length > 0 ? selectedFiles : [file]
  }

  const setWorkspaceClipboard = (
    mode: 'copy' | 'move',
    actionFiles: ConversationWorkspaceFileRead[],
  ) => {
    setClipboard({ mode, paths: actionFiles.map((file) => file.path) })
  }

  const pasteWorkspaceClipboard = async (destination: string) => {
    if (!clipboard) return
    setOperationError(null)
    actions.reset()
    try {
      await actions.mutateAsync({
        action: clipboard.mode,
        paths: clipboard.paths,
        destination,
      })
      setSelectedWorkspacePaths(new Set())
      if (clipboard.mode === 'move') setClipboard(null)
    } catch (error: unknown) {
      setOperationError(workspaceErrorMessageKey(error))
    }
  }

  const startMove = (actionFiles: ConversationWorkspaceFileRead[]) => {
    setMovePaths(actionFiles.map((file) => file.path))
    setMoveDestination('')
    setOperationError(null)
    actions.reset()
  }

  const submitMove = async () => {
    if (!movePaths) return
    setOperationError(null)
    actions.reset()
    try {
      await actions.mutateAsync({
        action: 'move',
        paths: movePaths,
        destination: moveDestination.trim(),
      })
      setMovePaths(null)
      setMoveDestination('')
      setSelectedWorkspacePaths(new Set())
    } catch (error: unknown) {
      setOperationError(workspaceErrorMessageKey(error))
    }
  }

  const performDelete = async (actionFiles: ConversationWorkspaceFileRead[]) => {
    const paths = actionFiles.map((file) => file.path)
    try {
      if (paths.length === 1) await del.mutateAsync(paths[0]!)
      else await actions.mutateAsync({ action: 'delete', paths })
    } catch (error: unknown) {
      throw new Error(
        t('common:workspaceOperations.deletePathError', {
          message: t(`chat:${workspaceErrorMessageKey(error)}`),
        }),
      )
    }
    const deletesPreview = actionFiles.some((file) => (
      previewFile?.path === file.path
      || Boolean(file.is_dir && previewFile?.path.startsWith(`${file.path}/`))
    ))
    if (deletesPreview) {
      setPreviewFile(null)
      setIsPreviewOpen(false)
    }
    if (activeConversationId) closeEditorPaths(activeConversationId, activeAgentId, actionFiles)
    setSelectedWorkspacePaths((current) => {
      const next = new Set<string>()
      for (const path of current) {
        if (actionFiles.some((file) => (
          path === file.path || (file.is_dir && path.startsWith(`${file.path}/`))
        ))) continue
        next.add(path)
      }
      return next
    })
  }

  const performClear = async () => {
    try {
      await actions.mutateAsync({ action: 'clear' })
    } catch (error: unknown) {
      throw new Error(t(`chat:${workspaceErrorMessageKey(error)}`))
    }
    setCurrentPath('')
    setSelectedWorkspacePaths(new Set())
    setClipboard(null)
    setPreviewFile(null)
    setIsPreviewOpen(false)
    if (activeConversationId) closeEditorPaths(activeConversationId, activeAgentId)
  }

  const menuFile = menu?.file ?? null
  const menuActionFiles = menuFile ? filesForAction(menuFile) : []
  const isSingleMenuAction = menuActionFiles.length === 1

  return (
    <div className="flex h-full min-h-0 flex-col">
      <span id={`${dragDescriptionId}-file`} className="sr-only">
        {t('chat:workspace.filePanel.dragFileDescription')}
      </span>
      <span id={`${dragDescriptionId}-directory`} className="sr-only">
        {t('chat:workspace.filePanel.dragDirectoryDescription')}
      </span>

      {rootEntries.length > 1 ? (
        <div className="shrink-0 space-y-1 border-b border-border px-3 py-2">
          <select
            aria-label={t('chat:workspace.rootPicker.label')}
            className="h-8 w-full rounded-md border border-input bg-background px-2 text-xs"
            value={activeAgentId ?? ''}
            onChange={(event) => setSelectedAgentId(event.target.value || null)}
          >
            {rootEntries.map((entry) => (
              <option key={entry.agent_id ?? 'conversation'} value={entry.agent_id ?? ''}>
                {entry.agent_id === null
                  ? t('chat:workspace.rootPicker.conversation')
                  : entry.is_primary
                    ? t('chat:workspace.rootPicker.agentPrimary', { name: entry.display_name })
                    : t('chat:workspace.rootPicker.agentMounted', { name: entry.display_name })}
              </option>
            ))}
          </select>
          {activeRoot ? (
            <p className="truncate text-2xs text-muted-foreground" title={activeRoot.root}>
              {activeRoot.root}
            </p>
          ) : null}
        </div>
      ) : null}

      <div className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-border px-2">
        <p
          className="flex min-w-0 items-center gap-1.5 truncate text-xs font-medium"
          title={activeRoot?.root ?? title}
        >
          <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          {title}
        </p>
        <div className="flex shrink-0 items-center gap-1">
          <div className="flex items-center rounded-md border border-border bg-muted/40 p-0.5">
            <Button
              type="button"
              variant={previewMode === 'dialog' ? 'secondary' : 'ghost'}
              size="icon"
              className="h-6 w-6"
              aria-label={t('chat:workspace.filePanel.popupMode')}
              title={t('chat:workspace.filePanel.popupMode')}
              aria-pressed={previewMode === 'dialog'}
              onClick={() => changePreviewMode('dialog')}
            >
              <AppWindow className="h-3.5 w-3.5" />
            </Button>
            <Button
              type="button"
              variant={previewMode === 'editor' ? 'secondary' : 'ghost'}
              size="icon"
              className="h-6 w-6"
              aria-label={t('chat:workspace.filePanel.editorMode')}
              title={t('chat:workspace.filePanel.editorMode')}
              aria-pressed={previewMode === 'editor'}
              onClick={() => changePreviewMode('editor')}
            >
              <PanelsTopLeft className="h-3.5 w-3.5" />
            </Button>
          </div>
          {canUpload ? (
            <>
              <input
                ref={fileInputRef}
                type="file"
                className="sr-only"
                onChange={(event) => uploadFile(event.target.files?.[0])}
                aria-label={t('chat:workspace.filePanel.uploadAria')}
              />
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 shrink-0"
                onClick={() => fileInputRef.current?.click()}
                disabled={upload.isPending || !hasConversation}
                aria-label={t('chat:workspace.filePanel.uploadAria')}
                title={t('chat:workspace.filePanel.uploadTitle')}
              >
                <Upload className="h-4 w-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
                onClick={() => setPendingClear(true)}
                disabled={actions.isPending || !hasConversation}
                aria-label={t('chat:workspace.fileActions.clearAria')}
                title={t('chat:workspace.fileActions.clearAria')}
              >
                <Eraser className="h-4 w-4" />
              </Button>
            </>
          ) : null}
          <Button
            type="button"
            variant={showHidden ? 'secondary' : 'ghost'}
            size="icon"
            className="h-7 w-7 shrink-0"
            onClick={toggleShowHidden}
            disabled={!hasConversation}
            aria-label={showHidden
              ? t('chat:workspace.filePanel.hideHidden')
              : t('chat:workspace.filePanel.showHidden')}
            title={showHidden
              ? t('chat:workspace.filePanel.hideHidden')
              : t('chat:workspace.filePanel.showHidden')}
            aria-pressed={showHidden}
          >
            {showHidden
              ? <EyeOff className="h-4 w-4" />
              : <Eye className="h-4 w-4" />}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0"
            onClick={() => void refreshFiles()}
            disabled={isRefreshing || !hasConversation}
            aria-label={t('chat:workspace.filePanel.refresh')}
          >
            <RefreshCw className={cn('h-4 w-4', isRefreshing && 'animate-spin')} />
          </Button>
        </div>
      </div>

      <div className="shrink-0 border-b border-border px-2 py-1.5">
        <SearchInput
          value={searchInput}
          onChange={changeSearchInput}
          label={t('chat:workspace.filePanel.search')}
          className="max-w-none sm:max-w-none"
        />
      </div>

      {currentPath && !isSearchMode ? (
        <button
          type="button"
          className="flex h-8 items-center gap-1.5 border-b border-border px-2 text-xs text-muted-foreground hover:bg-muted/70 hover:text-foreground"
          onClick={() => setCurrentPath(parentPath(currentPath))}
        >
          <ChevronLeft className="h-3.5 w-3.5" />
          {t('chat:workspace.filePanel.up')}
        </button>
      ) : null}

      {selectedCount > 0 || clipboard ? (
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground">
          <span>
            {selectedCount > 0
              ? t('common:workspaceOperations.selectedCount', {
                  count: selectedCount,
                  formattedCount: formatNumber(selectedCount, language),
                })
              : t('chat:workspace.fileActions.clipboardReady', {
                  count: clipboard?.paths.length ?? 0,
                  formattedCount: formatNumber(clipboard?.paths.length ?? 0, language),
                })}
          </span>
          <div className="flex items-center gap-0.5">
            {selectedCount > 0 ? (
              <>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={() => setWorkspaceClipboard('copy', selectedFiles)}
                  aria-label={t('chat:workspace.fileActions.copySelected')}
                  title={t('chat:workspace.fileActions.copySelected')}
                >
                  <Copy className="h-3.5 w-3.5" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={() => setWorkspaceClipboard('move', selectedFiles)}
                  aria-label={t('chat:workspace.fileActions.cutSelected')}
                  title={t('chat:workspace.fileActions.cutSelected')}
                >
                  <Scissors className="h-3.5 w-3.5" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={() => startMove(selectedFiles)}
                  aria-label={t('chat:workspace.fileActions.moveSelected')}
                  title={t('chat:workspace.fileActions.moveSelected')}
                >
                  <FolderInput className="h-3.5 w-3.5" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 hover:text-destructive"
                  onClick={() => setPendingDelete(selectedFiles)}
                  aria-label={t('chat:workspace.fileActions.deleteSelected')}
                  title={t('chat:workspace.fileActions.deleteSelected')}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </>
            ) : null}
            {clipboard ? (
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={() => void pasteWorkspaceClipboard(currentPath)}
                disabled={actions.isPending}
                aria-label={t('chat:workspace.fileActions.pasteHere')}
                title={t('chat:workspace.fileActions.pasteHere')}
              >
                <ClipboardPaste className="h-3.5 w-3.5" />
              </Button>
            ) : null}
          </div>
        </div>
      ) : null}

      {files.error ? (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
          {t('chat:workspace.filePanel.loadError', {
            message: t(`chat:${workspaceErrorMessageKey(files.error)}`),
          })}
        </div>
      ) : null}
      {canUpload && upload.error ? (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
          {t('chat:errors.uploadDetail', {
            message: t(`chat:${workspaceErrorMessageKey(upload.error)}`),
          })}
        </div>
      ) : null}
      {operationError ? (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
          {t('chat:workspace.filePanel.operationError', {
            message: t(`chat:${operationError}`),
          })}
        </div>
      ) : null}

      <div
        ref={fileListRef}
        className="min-h-0 flex-1 overflow-y-auto py-1"
        role="region"
        aria-label={t('chat:workspace.files')}
        onScroll={(event) => {
          setFileListFirstRow(Math.floor(event.currentTarget.scrollTop / FILE_ROW_HEIGHT))
        }}
        onClick={(event) => {
          if (event.target instanceof Element && event.target.closest('li')) return
          selectionAnchorRef.current = null
          setSelectedWorkspacePaths(new Set())
        }}
        onContextMenu={(event) => {
          if (event.target instanceof Element && event.target.closest('li')) return
          event.preventDefault()
          openContextMenu(event.clientX, event.clientY, null)
        }}
      >
        {!conversationId ? (
          <p className="p-3 text-sm text-muted-foreground">
            {t('chat:workspace.filePanel.selectConversation')}
          </p>
        ) : null}
        {conversationId && !workspaceId ? (
          <p className="p-3 text-sm text-muted-foreground">
            {t('chat:workspace.filePanel.noWorkspace')}
          </p>
        ) : null}
        {hasConversation && (files.isLoading || isSearchPending) ? (
          <p className="p-3 text-sm text-muted-foreground" role="status">
            {isSearchMode
              ? t('chat:workspace.filePanel.searching')
              : t('chat:workspace.loading')}
          </p>
        ) : null}
        {hasConversation
          && !files.isLoading
          && !isSearchPending
          && !files.error
          && sortedFiles.length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground">
            <Folder className="h-8 w-8" />
            <p>
              {isSearchMode
                ? t('chat:workspace.filePanel.noSearchMatches')
                : t('chat:workspace.empty')}
            </p>
          </div>
        ) : null}
        {sortedFiles.length > 0 && !isSearchPending ? (
          <ul className="relative" style={{ height: sortedFiles.length * FILE_ROW_HEIGHT }}>
            {visibleFiles.map((file, index) => {
              const absoluteIndex = visibleStart + index
              const isSelected = selectedWorkspacePaths.has(file.path)
              const kind = file.is_dir ? 'directory' : 'file'
              return (
                <li
                  key={file.path}
                  aria-posinset={absoluteIndex + 1}
                  aria-setsize={sortedFiles.length}
                  data-virtual-index={absoluteIndex}
                  className={cn(
                    'absolute inset-x-0 mx-1 rounded-sm hover:bg-muted/70',
                    isSelected && 'bg-muted text-foreground',
                    clipboard?.mode === 'move'
                      && clipboard.paths.includes(file.path)
                      && 'opacity-60',
                  )}
                  style={{ transform: `translateY(${absoluteIndex * FILE_ROW_HEIGHT}px)` }}
                  onContextMenu={(event) => {
                    event.preventDefault()
                    event.stopPropagation()
                    openContextMenu(event.clientX, event.clientY, file)
                  }}
                >
                  <button
                    ref={(element) => {
                      if (element) fileButtonRefs.current.set(file.path, element)
                      else fileButtonRefs.current.delete(file.path)
                    }}
                    type="button"
                    draggable
                    data-git-ignored={file.ignored || undefined}
                    className={cn(
                      'flex h-8 w-full min-w-0 cursor-grab select-none items-center gap-1.5 rounded-sm px-1.5 text-left active:cursor-grabbing focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                      file.ignored
                        && !isSelected
                        && 'opacity-60 hover:opacity-80 focus-visible:opacity-100',
                    )}
                    onPointerDown={(event) => {
                      didDragRef.current = false
                      pointerOriginRef.current = { x: event.clientX, y: event.clientY }
                    }}
                    onClick={(event) => handleFileClick(event, file)}
                    onKeyDown={(event) => handleFileKeyDown(event, file)}
                    onDragStart={(event) => handleFileDragStart(event, file)}
                    onDragEnd={() => setDraggingPath(null)}
                    aria-pressed={isSelected}
                    aria-grabbed={draggingPath === file.path}
                    aria-haspopup="menu"
                    aria-controls={menu?.file?.path === file.path ? contextMenuId : undefined}
                    aria-describedby={`${dragDescriptionId}-${kind}`}
                  >
                    {file.is_dir ? (
                      <>
                        <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
                        <Folder className="h-4 w-4 shrink-0 text-primary" />
                      </>
                    ) : (
                      <>
                        <span className="w-3 shrink-0" aria-hidden="true" />
                        <File className="h-4 w-4 shrink-0 text-muted-foreground" />
                      </>
                    )}
                    <span className="min-w-0 flex-1 truncate text-xs" title={file.path}>
                      {isSearchMode ? file.path : file.name}
                    </span>
                    {!file.is_dir ? (
                      <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">
                        {formatSize(file.size, language)}
                      </span>
                    ) : null}
                  </button>
                </li>
              )
            })}
          </ul>
        ) : null}
      </div>

      {pendingDelete ? (
        <ConfirmDialog
          open
          onOpenChange={(open) => {
            if (!open) setPendingDelete(null)
          }}
          title={pendingDelete.length === 1
            ? t('chat:workspace.deleteTitle', { path: pendingDelete[0]?.path })
            : t('chat:workspace.fileActions.deleteManyTitle', {
                count: pendingDelete.length,
                formattedCount: formatNumber(pendingDelete.length, language),
              })}
          description={
            pendingDelete.length > 1
              ? t('chat:workspace.fileActions.deleteManyDescription')
              : pendingDelete[0]?.is_dir
                ? t('chat:workspace.filePanel.deleteFolderDescription')
                : t('chat:workspace.filePanel.deleteFileDescription')
          }
          confirmLabel={t('common:actions.delete')}
          destructive
          onConfirm={() => performDelete(pendingDelete)}
        />
      ) : null}

      {pendingClear ? (
        <ConfirmDialog
          open
          onOpenChange={setPendingClear}
          title={t('chat:workspace.fileActions.clearTitle')}
          description={t('chat:workspace.fileActions.clearDescription')}
          confirmLabel={t('chat:workspace.fileActions.clearConfirm')}
          destructive
          onConfirm={performClear}
        />
      ) : null}

      <Dialog
        open={movePaths !== null}
        onOpenChange={(open) => {
          if (!open && !actions.isPending) {
            setMovePaths(null)
            setMoveDestination('')
          }
        }}
      >
        <DialogContent closeLabel={t('common:actions.close')} className="sm:max-w-md">
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault()
              void submitMove()
            }}
          >
            <DialogHeader>
              <DialogTitle>{t('chat:workspace.fileActions.moveTitle')}</DialogTitle>
              <DialogDescription>
                {t('chat:workspace.fileActions.moveDescription')}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="workspace-move-destination">
                {t('chat:workspace.fileActions.destinationFolder')}
              </label>
              <Input
                id="workspace-move-destination"
                value={moveDestination}
                onChange={(event) => setMoveDestination(event.target.value)}
                placeholder={t('chat:workspace.fileActions.destinationPlaceholder')}
                autoFocus
              />
              {actions.error ? (
                <p className="text-xs text-destructive" role="alert">
                  {t('chat:workspace.filePanel.operationError', {
                    message: t(`chat:${workspaceErrorMessageKey(actions.error)}`),
                  })}
                </p>
              ) : null}
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setMovePaths(null)}
                disabled={actions.isPending}
              >
                {t('common:actions.cancel')}
              </Button>
              <Button type="submit" disabled={actions.isPending}>
                {t('chat:workspace.fileActions.moveConfirm')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={creating !== null}
        onOpenChange={(open) => {
          if (!open && !create.isPending) {
            setCreating(null)
            setCreateName('')
          }
        }}
      >
        <DialogContent closeLabel={t('common:actions.close')} className="sm:max-w-md">
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault()
              void submitCreate()
            }}
          >
            <DialogHeader>
              <DialogTitle>
                {creating?.kind === 'directory'
                  ? t('chat:workspace.fileActions.newFolderTitle')
                  : t('chat:workspace.fileActions.newFileTitle')}
              </DialogTitle>
              <DialogDescription>
                {creating?.parent
                  ? t('chat:workspace.fileActions.createInFolder', { path: creating.parent })
                  : t('chat:workspace.fileActions.createInRoot')}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="workspace-create-name">
                {t('chat:workspace.fileActions.createName')}
              </label>
              <Input
                id="workspace-create-name"
                value={createName}
                onChange={(event) => setCreateName(event.target.value)}
                placeholder={creating?.kind === 'directory'
                  ? t('chat:workspace.fileActions.newFolderPlaceholder')
                  : t('chat:workspace.fileActions.newFilePlaceholder')}
                autoFocus
              />
              {create.error ? (
                <p className="text-xs text-destructive" role="alert">
                  {t('chat:workspace.filePanel.operationError', {
                    message: t(`chat:${workspaceErrorMessageKey(create.error)}`),
                  })}
                </p>
              ) : null}
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setCreating(null)}
                disabled={create.isPending}
              >
                {t('common:actions.cancel')}
              </Button>
              <Button type="submit" disabled={create.isPending || !createName.trim()}>
                {t('chat:workspace.fileActions.createConfirm')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={isPreviewOpen && previewFile !== null} onOpenChange={setIsPreviewOpen}>
        <DialogContent
          closeLabel={t('common:actions.close')}
          className="flex h-[min(88vh,52rem)] w-[min(94vw,72rem)] max-w-none flex-col gap-0 overflow-hidden p-0"
        >
          <DialogHeader className="shrink-0 border-b border-border px-5 py-3.5 pr-12">
            <DialogTitle
              aria-label={previewFile?.path ?? t('chat:workspace.preview')}
              className="flex min-w-0 items-center gap-2 text-base"
            >
              <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                <File className="h-4 w-4" aria-hidden="true" />
              </span>
              <span className="truncate" title={previewFile?.path}>
                {previewFile?.name ?? t('chat:workspace.preview')}
              </span>
            </DialogTitle>
            <DialogDescription className="flex min-w-0 items-center gap-2 pl-10 text-xs">
              {previewFile?.path ? (
                <span className="min-w-0 truncate font-mono text-2xs" title={previewFile.path}>
                  {previewFile.path}
                </span>
              ) : null}
              {previewFile?.size != null ? (
                <>
                  <span aria-hidden="true">·</span>
                  <span className="shrink-0">{formatSize(previewFile.size, language)}</span>
                </>
              ) : null}
            </DialogDescription>
          </DialogHeader>
          <div className="flex min-h-0 flex-1 flex-col overflow-y-auto bg-muted/15 p-4 sm:p-5">
            {previewFile && activeConversationId ? (
              <WorkspacePreviewRouter
                scope={scope}
                conversationId={activeConversationId}
                file={previewFile}
                agentId={activeAgentId}
              />
            ) : null}
          </div>
        </DialogContent>
      </Dialog>

      {renaming ? (
        <div className="border-t border-border p-3">
          <label className="mb-1 block text-xs font-medium" htmlFor="workspace-file-rename">
            {t('chat:workspace.filePanel.renamePath')}
          </label>
          <div className="flex gap-2">
            <Input
              id="workspace-file-rename"
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              className="h-8 text-xs"
            />
            <Button size="sm" onClick={submitRename} disabled={rename.isPending}>
              {t('common:actions.save')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setRenaming(null)}>
              {t('common:actions.cancel')}
            </Button>
          </div>
          {rename.error ? (
            <p className="mt-2 text-xs text-destructive" role="alert">
              {t('chat:workspace.filePanel.renameError', {
                message: t(`chat:${workspaceErrorMessageKey(rename.error)}`),
              })}
            </p>
          ) : null}
        </div>
      ) : null}

      {menu ? (
        <div
          id={contextMenuId}
          className="fixed z-50 min-w-44 overflow-hidden rounded-md border border-border bg-background py-1 text-sm text-foreground shadow-md"
          style={{ top: menu.y, left: menu.x }}
          role="menu"
          aria-label={t('chat:workspace.contextMenu')}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={handleMenuKeyDown}
        >
          {menuFile && isSingleMenuAction ? (
            <button
              ref={menuFirstItemRef}
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              onClick={() => {
                if (menuFile.is_dir) openEntry(menuFile)
                else openFile(menuFile, 'dialog')
                setMenu(null)
              }}
            >
              {menuFile.is_dir ? (
                <FolderOpen className="h-3.5 w-3.5" />
              ) : (
                <File className="h-3.5 w-3.5" />
              )}
              {menuFile.is_dir
                ? t('chat:workspace.filePanel.openFolder')
                : t('chat:workspace.filePanel.openPopup')}
            </button>
          ) : null}
          {menuFile && isSingleMenuAction && !menuFile.is_dir ? (
            <button
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              onClick={() => {
                openFile(menuFile, 'editor')
                setMenu(null)
              }}
            >
              <PanelsTopLeft className="h-3.5 w-3.5" />
              {t('chat:workspace.filePanel.openEditor')}
            </button>
          ) : null}
          {menuFile && isSingleMenuAction && !menuFile.is_dir ? (
            <button
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              disabled={downloadingPath === menuFile.path}
              onClick={() => {
                downloadFile(menuFile)
                setMenu(null)
              }}
            >
              <Download className="h-3.5 w-3.5" />
              {t('chat:workspace.download')}
            </button>
          ) : null}
          {menuFile && isSingleMenuAction && canRevealInFileManager && menuFile.abs_path ? (
            <button
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              onClick={() => {
                revealFile(menuFile)
                setMenu(null)
              }}
            >
              <FolderOpen className="h-3.5 w-3.5" />
              {t('chat:workspace.reveal')}
            </button>
          ) : null}
          {canMutate && menuFile ? (
            <>
              {isSingleMenuAction && menuFile.is_dir ? (
                <>
                  <button
                    type="button"
                    role="menuitem"
                    className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                    onClick={() => {
                      startCreate('file', menuFile.path)
                      setMenu(null)
                    }}
                  >
                    <FilePlus className="h-3.5 w-3.5" />
                    {t('chat:workspace.fileActions.newFile')}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                    onClick={() => {
                      startCreate('directory', menuFile.path)
                      setMenu(null)
                    }}
                  >
                    <FolderPlus className="h-3.5 w-3.5" />
                    {t('chat:workspace.fileActions.newFolder')}
                  </button>
                </>
              ) : null}
              {isSingleMenuAction ? (
                <button
                  type="button"
                  role="menuitem"
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                  onClick={() => {
                    startRename(menuFile)
                    setMenu(null)
                  }}
                >
                  <Pencil className="h-3.5 w-3.5" />
                  {t('chat:workspace.rename')}
                </button>
              ) : null}
              <div className="my-1 border-t border-border" role="separator" />
              <button
                ref={isSingleMenuAction ? undefined : menuFirstItemRef}
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                onClick={() => {
                  setWorkspaceClipboard('copy', menuActionFiles)
                  setMenu(null)
                }}
              >
                <Copy className="h-3.5 w-3.5" />
                {t('chat:workspace.fileActions.copy')}
              </button>
              <button
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                onClick={() => {
                  setWorkspaceClipboard('move', menuActionFiles)
                  setMenu(null)
                }}
              >
                <Scissors className="h-3.5 w-3.5" />
                {t('chat:workspace.fileActions.cut')}
              </button>
              {clipboard ? (
                <button
                  type="button"
                  role="menuitem"
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted disabled:opacity-50"
                  disabled={actions.isPending}
                  onClick={() => {
                    const destination = menuFile.is_dir ? menuFile.path : currentPath
                    void pasteWorkspaceClipboard(destination)
                    setMenu(null)
                  }}
                >
                  <ClipboardPaste className="h-3.5 w-3.5" />
                  {menuFile.is_dir
                    ? t('chat:workspace.fileActions.pasteInto', { name: menuFile.name })
                    : t('chat:workspace.fileActions.pasteHere')}
                </button>
              ) : null}
              <button
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                onClick={() => {
                  startMove(menuActionFiles)
                  setMenu(null)
                }}
              >
                <FolderInput className="h-3.5 w-3.5" />
                {t('chat:workspace.fileActions.moveTo')}
              </button>
              <button
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-destructive hover:bg-muted"
                onClick={() => {
                  setPendingDelete(menuActionFiles)
                  setMenu(null)
                }}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t('common:actions.delete')}
              </button>
            </>
          ) : null}
          {!menuFile ? (
            <>
              {canMutate ? (
                <>
                  <button
                    ref={menuFirstItemRef}
                    type="button"
                    role="menuitem"
                    className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                    onClick={() => {
                      startCreate('file', currentPath)
                      setMenu(null)
                    }}
                  >
                    <FilePlus className="h-3.5 w-3.5" />
                    {t('chat:workspace.fileActions.newFile')}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                    onClick={() => {
                      startCreate('directory', currentPath)
                      setMenu(null)
                    }}
                  >
                    <FolderPlus className="h-3.5 w-3.5" />
                    {t('chat:workspace.fileActions.newFolder')}
                  </button>
                  <div className="my-1 border-t border-border" role="separator" />
                </>
              ) : null}
              {canUpload ? (
                <button
                  ref={canMutate ? undefined : menuFirstItemRef}
                  type="button"
                  role="menuitem"
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                  onClick={() => {
                    fileInputRef.current?.click()
                    setMenu(null)
                  }}
                >
                  <Upload className="h-3.5 w-3.5" />
                  {t('chat:workspace.upload')}
                </button>
              ) : null}
              {clipboard ? (
                <button
                  ref={canMutate || canUpload ? undefined : menuFirstItemRef}
                  type="button"
                  role="menuitem"
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted disabled:opacity-50"
                  disabled={actions.isPending}
                  onClick={() => {
                    void pasteWorkspaceClipboard(currentPath)
                    setMenu(null)
                  }}
                >
                  <ClipboardPaste className="h-3.5 w-3.5" />
                  {t('chat:workspace.fileActions.pasteHere')}
                </button>
              ) : null}
              <button
                ref={!canMutate && !canUpload && !clipboard ? menuFirstItemRef : undefined}
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                onClick={() => {
                  void refreshFiles()
                  setMenu(null)
                }}
              >
                <RefreshCw className="h-3.5 w-3.5" />
                {t('chat:workspace.refresh')}
              </button>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
