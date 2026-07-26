import { useEffect, useId, useRef, useState } from 'react'
import {
  ChevronLeft,
  Download,
  File,
  Folder,
  FolderOpen,
  Pencil,
  RefreshCw,
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
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  useConversationWorkspaceFiles,
  useConversationWorkspaceRoots,
  useDownloadConversationWorkspaceFile,
  useUploadConversationWorkspaceFile,
} from '@/hooks/useConversationWorkspaceFiles'
import {
  useDeleteGroupWorkspaceFile,
  useRenameGroupWorkspaceFile,
} from '@/hooks/useGroupFiles'
import { normalizeLanguage } from '@/i18n'
import {
  workspaceErrorMessageKey,
  type WorkspaceErrorMessageKey,
} from '@/i18n/localizedError'
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
  onInsertPaths?: (paths: string[]) => void
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

export function WorkspaceFilesTab({
  scope,
  conversationId,
  workspaceId,
  onInsertPaths,
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
  const [renaming, setRenaming] = useState<ConversationWorkspaceFileRead | null>(null)
  const [pendingDelete, setPendingDelete] = useState<ConversationWorkspaceFileRead | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [operationError, setOperationError] = useState<WorkspaceErrorMessageKey | null>(null)
  const [downloadingPath, setDownloadingPath] = useState<string | null>(null)
  const [draggingPath, setDraggingPath] = useState<string | null>(null)
  const [menu, setMenu] = useState<{
    x: number
    y: number
    file: ConversationWorkspaceFileRead
  } | null>(null)
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const fileButtonRefs = useRef(new Map<string, HTMLButtonElement>())
  const menuFirstItemRef = useRef<HTMLButtonElement | null>(null)
  const dragDescriptionId = useId()
  const contextMenuId = useId()
  const activeConversationId = workspaceId ? conversationId : undefined
  const hasConversation = Boolean(activeConversationId)
  const canUpload = hasConversation
  const canMutate = hasConversation
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
    currentPath,
    activeAgentId,
  )
  const upload = useUploadConversationWorkspaceFile(scope, activeConversationId, activeAgentId)
  const download = useDownloadConversationWorkspaceFile(scope, activeConversationId, activeAgentId)
  const rename = useRenameGroupWorkspaceFile(activeConversationId, scope, activeAgentId)
  const del = useDeleteGroupWorkspaceFile(activeConversationId, scope, activeAgentId)
  const navRequest = useFileNavStore((state) => state.request)
  const clearNav = useFileNavStore((state) => state.clear)

  const title = currentPath || t('chat:workspace.root')
  const sortedFiles = files.data ?? []
  const selectedCount = selectedWorkspacePaths.size

  const selectOnlyPath = (path: string) => setSelectedWorkspacePaths(new Set([path]))

  const toggleSelectedPath = (path: string) => {
    setSelectedWorkspacePaths((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const selectedDragFiles = (file: ConversationWorkspaceFileRead) => {
    if (!selectedWorkspacePaths.has(file.path)) return [file]
    const selected = sortedFiles.filter((candidate) => selectedWorkspacePaths.has(candidate.path))
    return selected.length > 0 ? selected : [file]
  }

  const openEntry = (file: ConversationWorkspaceFileRead) => {
    selectOnlyPath(file.path)
    if (file.is_dir) {
      setCurrentPath(file.path)
      return
    }
    setPreviewFile(file)
    setIsPreviewOpen(true)
  }

  const handleFileClick = (
    event: React.MouseEvent<HTMLButtonElement>,
    file: ConversationWorkspaceFileRead,
  ) => {
    if (event.ctrlKey || event.metaKey) {
      toggleSelectedPath(file.path)
      return
    }
    openEntry(file)
  }

  const handleFileKeyDown = (
    event: React.KeyboardEvent<HTMLButtonElement>,
    file: ConversationWorkspaceFileRead,
  ) => {
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return
    event.preventDefault()
    const rect = event.currentTarget.getBoundingClientRect()
    setMenu({ x: rect.left + 8, y: rect.bottom, file })
  }

  const handleFileDragStart = (
    event: React.DragEvent<HTMLButtonElement>,
    file: ConversationWorkspaceFileRead,
  ) => {
    const draggedFiles = selectedDragFiles(file)
    if (!selectedWorkspacePaths.has(file.path)) selectOnlyPath(file.path)
    setDraggingPath(file.path)
    event.dataTransfer.effectAllowed = 'copy'
    event.dataTransfer.setData(
      WORKSPACE_ITEM_MIME,
      encodeWorkspaceDragItems(draggedFiles.map(dragItem)),
    )
    event.dataTransfer.setData('text/plain', draggedFiles.map((item) => item.path).join('\n'))
  }

  const insertSelectedPaths = () => {
    if (selectedWorkspacePaths.size === 0) return
    onInsertPaths?.(Array.from(selectedWorkspacePaths))
  }

  useEffect(() => {
    setCurrentPath('')
    setPreviewFile(null)
    setIsPreviewOpen(false)
    setRenaming(null)
    setPendingDelete(null)
    setOperationError(null)
    setMenu(null)
  }, [conversationId, scope, workspaceId])

  useEffect(() => {
    if (!navRequest || navRequest.groupId !== conversationId || !workspaceId) return
    setSelectedAgentId(navRequest.agentId ?? null)
    // An empty path means "show me this root", not "open this file".
    if (!navRequest.path) {
      setCurrentPath('')
      setPreviewFile(null)
      setIsPreviewOpen(false)
      clearNav()
      return
    }
    const requestedFile: ConversationWorkspaceFileRead = {
      path: navRequest.path,
      name: fileName(navRequest.path),
      is_dir: false,
      size: null,
      modified_at: null,
    }
    setCurrentPath(parentPath(navRequest.path))
    setPreviewFile(requestedFile)
    setIsPreviewOpen(true)
    clearNav()
  }, [clearNav, conversationId, navRequest, workspaceId])

  useEffect(() => {
    setSelectedWorkspacePaths(new Set())
  }, [currentPath, conversationId, scope])

  useEffect(() => {
    setCurrentPath('')
    setPreviewFile(null)
    setIsPreviewOpen(false)
  }, [activeAgentId])

  useEffect(() => {
    if (!menu) return
    menuFirstItemRef.current?.focus()
    const close = () => setMenu(null)
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      const trigger = fileButtonRefs.current.get(menu.file.path)
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
      const trigger = fileButtonRefs.current.get(menu.file.path)
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
      .then(() => setCurrentPath('uploads'))
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

  const performDelete = async (file: ConversationWorkspaceFileRead) => {
    try {
      await del.mutateAsync(file.path)
    } catch (error: unknown) {
      throw new Error(
        t('common:workspaceOperations.deletePathError', {
          message: t(`chat:${workspaceErrorMessageKey(error)}`),
        }),
      )
    }
    const deletesPreview = previewFile?.path === file.path
      || Boolean(file.is_dir && previewFile?.path.startsWith(`${file.path}/`))
    if (deletesPreview) {
      setPreviewFile(null)
      setIsPreviewOpen(false)
    }
    setSelectedWorkspacePaths((current) => {
      const next = new Set<string>()
      for (const path of current) {
        if (path === file.path) continue
        if (file.is_dir && path.startsWith(`${file.path}/`)) continue
        next.add(path)
      }
      return next
    })
  }

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
            <p className="truncate text-[11px] text-muted-foreground" title={activeRoot.root}>
              {activeRoot.root}
            </p>
          ) : null}
        </div>
      ) : null}

      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
        <p className="min-w-0 truncate text-[11px] text-muted-foreground" title={title}>
          {title}
        </p>
        <div className="flex shrink-0 items-center gap-1">
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
                className="h-8 w-8 shrink-0"
                onClick={() => fileInputRef.current?.click()}
                disabled={upload.isPending || !hasConversation}
                aria-label={t('chat:workspace.filePanel.uploadAria')}
                title={t('chat:workspace.filePanel.uploadTitle')}
              >
                <Upload className="h-4 w-4" />
              </Button>
            </>
          ) : null}
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={() => void files.refetch()}
            disabled={files.isFetching || !hasConversation}
            aria-label={t('chat:workspace.filePanel.refresh')}
          >
            <RefreshCw className={cn('h-4 w-4', files.isFetching && 'animate-spin')} />
          </Button>
        </div>
      </div>

      {currentPath ? (
        <button
          type="button"
          className="flex items-center gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground hover:bg-muted/70 hover:text-foreground"
          onClick={() => setCurrentPath(parentPath(currentPath))}
        >
          <ChevronLeft className="h-3.5 w-3.5" />
          {t('chat:workspace.filePanel.up')}
        </button>
      ) : null}

      {selectedCount > 0 ? (
        <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground">
          <span>
            {t('common:workspaceOperations.selectedCount', {
              count: selectedCount,
              formattedCount: formatNumber(selectedCount, language),
            })}
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 px-2 text-[11px]"
            onClick={insertSelectedPaths}
            disabled={!onInsertPaths}
          >
            {t('chat:workspace.insertSelected')}
          </Button>
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

      <div className="min-h-0 flex-1 overflow-y-auto">
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
        {hasConversation && files.isLoading ? (
          <p className="p-3 text-sm text-muted-foreground" role="status">
            {t('chat:workspace.loading')}
          </p>
        ) : null}
        {hasConversation && !files.isLoading && !files.error && sortedFiles.length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground">
            <Folder className="h-8 w-8" />
            <p>{t('chat:workspace.empty')}</p>
          </div>
        ) : null}
        {sortedFiles.length > 0 ? (
          <ul className="divide-y divide-border">
            {sortedFiles.map((file) => {
              const isSelected = selectedWorkspacePaths.has(file.path)
              const kind = file.is_dir ? 'directory' : 'file'
              return (
                <li
                  key={file.path}
                  className={cn(
                    'group flex items-center gap-2 px-3 py-2 hover:bg-muted/70',
                    isSelected && 'bg-muted ring-1 ring-inset ring-ring/40',
                  )}
                  onContextMenu={(event) => {
                    event.preventDefault()
                    setMenu({ x: event.clientX, y: event.clientY, file })
                  }}
                >
                  <button
                    ref={(element) => {
                      if (element) fileButtonRefs.current.set(file.path, element)
                      else fileButtonRefs.current.delete(file.path)
                    }}
                    type="button"
                    draggable
                    className="flex min-w-0 flex-1 items-center gap-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onClick={(event) => handleFileClick(event, file)}
                    onDoubleClick={() => openEntry(file)}
                    onKeyDown={(event) => handleFileKeyDown(event, file)}
                    onDragStart={(event) => handleFileDragStart(event, file)}
                    onDragEnd={() => setDraggingPath(null)}
                    aria-pressed={isSelected}
                    aria-grabbed={draggingPath === file.path}
                    aria-haspopup="menu"
                    aria-controls={menu?.file.path === file.path ? contextMenuId : undefined}
                    aria-describedby={`${dragDescriptionId}-${kind}`}
                  >
                    {file.is_dir ? (
                      <Folder className="h-4 w-4 shrink-0 text-primary" />
                    ) : (
                      <File className="h-4 w-4 shrink-0 text-muted-foreground" />
                    )}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">{file.name}</span>
                      <span className="block text-[10px] text-muted-foreground">
                        {file.is_dir
                          ? t('chat:workspace.filePanel.folder')
                          : formatSize(file.size, language)}
                      </span>
                    </span>
                  </button>
                  {!file.is_dir ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0 text-muted-foreground opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100"
                      onClick={() => downloadFile(file)}
                      disabled={downloadingPath === file.path}
                      aria-label={t('chat:workspace.filePanel.downloadNamed', { name: file.name })}
                    >
                      <Download className="h-3.5 w-3.5" />
                    </Button>
                  ) : null}
                  {canMutate ? (
                    <>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 shrink-0 text-muted-foreground opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100"
                        onClick={() => startRename(file)}
                        aria-label={t('chat:workspace.filePanel.renameNamed', { name: file.name })}
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 shrink-0 text-muted-foreground opacity-100 hover:text-destructive sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100"
                        onClick={() => setPendingDelete(file)}
                        disabled={del.isPending}
                        aria-label={t('chat:workspace.filePanel.deleteNamed', { name: file.name })}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </>
                  ) : null}
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
          title={t('chat:workspace.deleteTitle', { path: pendingDelete.path })}
          description={
            pendingDelete.is_dir
              ? t('chat:workspace.filePanel.deleteFolderDescription')
              : t('chat:workspace.filePanel.deleteFileDescription')
          }
          confirmLabel={t('common:actions.delete')}
          destructive
          onConfirm={() => performDelete(pendingDelete)}
        />
      ) : null}

      <Dialog open={isPreviewOpen && previewFile !== null} onOpenChange={setIsPreviewOpen}>
        <DialogContent
          closeLabel={t('common:actions.close')}
          className="max-h-[88vh] max-w-4xl overflow-hidden p-0"
        >
          <DialogHeader className="border-b border-border px-6 py-4 pr-12">
            <DialogTitle className="truncate text-base">
              {previewFile?.path ?? t('chat:workspace.preview')}
            </DialogTitle>
            <DialogDescription className="flex flex-wrap items-center gap-x-2 gap-y-1">
              {previewFile?.size != null ? <span>{formatSize(previewFile.size, language)}</span> : null}
              {previewFile?.size != null ? <span aria-hidden="true">·</span> : null}
              <span>{t('chat:workspace.filePanel.previewDescription')}</span>
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-40 overflow-y-auto px-6 py-4">
            {previewFile && activeConversationId ? (
              <WorkspacePreviewRouter
                scope={scope}
                conversationId={activeConversationId}
                file={previewFile}
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
          <button
            ref={menuFirstItemRef}
            type="button"
            role="menuitem"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
            onClick={() => {
              openEntry(menu.file)
              setMenu(null)
            }}
          >
            {menu.file.is_dir ? (
              <FolderOpen className="h-3.5 w-3.5" />
            ) : (
              <File className="h-3.5 w-3.5" />
            )}
            {menu.file.is_dir
              ? t('chat:workspace.filePanel.openFolder')
              : t('chat:workspace.filePanel.openPreview')}
          </button>
          {!menu.file.is_dir ? (
            <button
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              onClick={() => {
                downloadFile(menu.file)
                setMenu(null)
              }}
            >
              <Download className="h-3.5 w-3.5" />
              {t('chat:workspace.download')}
            </button>
          ) : null}
          {canMutate ? (
            <>
              <button
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
                onClick={() => {
                  startRename(menu.file)
                  setMenu(null)
                }}
              >
                <Pencil className="h-3.5 w-3.5" />
                {t('chat:workspace.rename')}
              </button>
              <button
                type="button"
                role="menuitem"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-destructive hover:bg-muted"
                onClick={() => {
                  setPendingDelete(menu.file)
                  setMenu(null)
                }}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t('common:actions.delete')}
              </button>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
