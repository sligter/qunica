import { useEffect, useRef, useState } from 'react'
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

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  downloadGroupWorkspaceFile,
  useDeleteGroupWorkspaceFile,
  useGroupWorkspaceFilePreview,
  useGroupWorkspaceFiles,
  useGroupWorkspaceRoot,
  useRenameGroupWorkspaceFile,
  useUploadGroupWorkspaceFile,
} from '@/hooks/useGroupFiles'
import { isDesktopRuntime, revealInFileManager } from '@/lib/desktop'
import { cn } from '@/lib/utils'
import { encodeWorkspacePaths, WORKSPACE_PATHS_MIME } from '@/lib/workspaceDrag'
import { joinWorkspaceAbsPath } from '@/lib/workspaceFileLink'
import { useAuthStore } from '@/stores/authStore'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { GroupWorkspaceFileRead } from '@/types/api'

interface WorkspaceFilesTabProps {
  groupId: string | undefined
  onInsertPaths?: (paths: string[]) => void
}

function parentPath(path: string) {
  const parts = path.split('/').filter(Boolean)
  parts.pop()
  return parts.join('/')
}

function formatSize(size: number | null | undefined) {
  if (size == null) return ''
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

function displayError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export function WorkspaceFilesTab({ groupId, onInsertPaths }: WorkspaceFilesTabProps) {
  const [currentPath, setCurrentPath] = useState('')
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [selectedWorkspacePaths, setSelectedWorkspacePaths] = useState<Set<string>>(
    () => new Set(),
  )
  const [isPreviewOpen, setIsPreviewOpen] = useState(false)
  const [renaming, setRenaming] = useState<GroupWorkspaceFileRead | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [downloadError, setDownloadError] = useState<string | null>(null)
  const [downloadingPath, setDownloadingPath] = useState<string | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; file: GroupWorkspaceFileRead } | null>(
    null,
  )
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const token = useAuthStore((s) => s.token)
  const files = useGroupWorkspaceFiles(groupId, currentPath)
  const preview = useGroupWorkspaceFilePreview(groupId, selectedPath)
  const root = useGroupWorkspaceRoot(groupId)
  const upload = useUploadGroupWorkspaceFile(groupId)
  const rename = useRenameGroupWorkspaceFile(groupId)
  const del = useDeleteGroupWorkspaceFile(groupId)
  const navRequest = useFileNavStore((s) => s.request)
  const clearNav = useFileNavStore((s) => s.clear)
  const desktop = isDesktopRuntime()
  const hasGroupId = groupId !== undefined && groupId.length > 0

  const title = currentPath || 'Workspace root'
  const sortedFiles = files.data ?? []
  const selectedCount = selectedWorkspacePaths.size

  const selectOnlyPath = (path: string) => {
    setSelectedWorkspacePaths(new Set([path]))
  }

  const toggleSelectedPath = (path: string) => {
    setSelectedWorkspacePaths((current) => {
      const next = new Set(current)
      if (next.has(path)) {
        next.delete(path)
      } else {
        next.add(path)
      }
      return next
    })
  }

  const fileDragPaths = (file: GroupWorkspaceFileRead) => {
    if (selectedWorkspacePaths.has(file.path)) return Array.from(selectedWorkspacePaths)
    return [file.path]
  }

  const openEntry = (file: GroupWorkspaceFileRead) => {
    selectOnlyPath(file.path)
    if (file.is_dir) {
      setCurrentPath(file.path)
    } else {
      setSelectedPath(file.path)
      setIsPreviewOpen(true)
    }
  }

  const absPathFor = (file: GroupWorkspaceFileRead) =>
    file.abs_path ?? joinWorkspaceAbsPath(root.data?.root, root.data?.separator, file.path)

  const revealEntry = (file: GroupWorkspaceFileRead) => {
    setDownloadError(null)
    void revealInFileManager(absPathFor(file)).catch((error: unknown) =>
      setDownloadError(displayError(error)),
    )
  }

  const handleFileClick = (
    event: React.MouseEvent<HTMLButtonElement>,
    file: GroupWorkspaceFileRead,
  ) => {
    if (event.ctrlKey || event.metaKey) {
      toggleSelectedPath(file.path)
      return
    }
    openEntry(file)
  }

  const handleFileDragStart = (
    event: React.DragEvent<HTMLLIElement>,
    file: GroupWorkspaceFileRead,
  ) => {
    const paths = fileDragPaths(file)
    if (!selectedWorkspacePaths.has(file.path)) {
      selectOnlyPath(file.path)
    }
    event.dataTransfer.effectAllowed = 'copy'
    event.dataTransfer.setData(WORKSPACE_PATHS_MIME, encodeWorkspacePaths(paths))
    event.dataTransfer.setData('text/plain', paths.join('\n'))
  }

  const insertSelectedPaths = () => {
    if (selectedWorkspacePaths.size === 0) return
    onInsertPaths?.(Array.from(selectedWorkspacePaths))
  }

  // Open a file when a chat link requests it (locate folder + preview).
  useEffect(() => {
    if (!navRequest || navRequest.groupId !== groupId) return
    setCurrentPath(parentPath(navRequest.path))
    setSelectedPath(navRequest.path)
    setIsPreviewOpen(true)
    clearNav()
  }, [navRequest, groupId, clearNav])

  useEffect(() => {
    setSelectedWorkspacePaths(new Set())
  }, [currentPath, groupId])

  // Dismiss the right-click menu on any outside interaction.
  useEffect(() => {
    if (!menu) return
    const close = () => setMenu(null)
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setMenu(null)
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

  const startRename = (file: GroupWorkspaceFileRead) => {
    setRenaming(file)
    setRenameValue(file.path)
  }

  const submitRename = () => {
    if (!hasGroupId || !renaming || !renameValue.trim()) return
    const oldPath = renaming.path
    const wasPreviewOpen = isPreviewOpen && selectedPath !== null
    void rename
      .mutateAsync({ path: oldPath, newPath: renameValue.trim() })
      .then((next) => {
        setRenaming(null)
        setSelectedPath((path) => {
          if (path === oldPath) return next.path
          if (path?.startsWith(`${oldPath}/`)) return `${next.path}${path.slice(oldPath.length)}`
          return path
        })
        setSelectedWorkspacePaths((current) => {
          const updated = new Set<string>()
          for (const path of current) {
            if (path === oldPath) {
              updated.add(next.path)
            } else if (path.startsWith(`${oldPath}/`)) {
              updated.add(`${next.path}${path.slice(oldPath.length)}`)
            } else {
              updated.add(path)
            }
          }
          return updated
        })
        setIsPreviewOpen(wasPreviewOpen)
        if (parentPath(oldPath) !== parentPath(next.path)) {
          setCurrentPath(parentPath(next.path))
        }
      })
  }

  const uploadFile = (file: File | undefined) => {
    if (!file || !hasGroupId) return
    setDownloadError(null)
    void upload
      .mutateAsync(file)
      .then(() => {
        setCurrentPath('uploads')
        if (fileInputRef.current) fileInputRef.current.value = ''
      })
      .catch(() => {
        if (fileInputRef.current) fileInputRef.current.value = ''
      })
  }

  const downloadFile = (file: GroupWorkspaceFileRead) => {
    if (!hasGroupId || file.is_dir) return
    setDownloadError(null)
    setDownloadingPath(file.path)
    void downloadGroupWorkspaceFile(groupId, file.path, token)
      .catch((error: unknown) => setDownloadError(displayError(error)))
      .finally(() => setDownloadingPath(null))
  }

  const deletePath = (file: GroupWorkspaceFileRead) => {
    if (!hasGroupId) return
    const confirmed = window.confirm(`Delete ${file.path}?`)
    if (!confirmed) return
    void del.mutateAsync(file.path).then(() => {
      const deletesSelectedPath =
        selectedPath === file.path || (file.is_dir && selectedPath?.startsWith(`${file.path}/`))
      setSelectedPath((selected) => {
        if (selected === file.path) return null
        if (file.is_dir && selected?.startsWith(`${file.path}/`)) return null
        return selected
      })
      setSelectedWorkspacePaths((current) => {
        const next = new Set<string>()
        for (const path of current) {
          if (path === file.path) continue
          if (file.is_dir && path.startsWith(`${file.path}/`)) continue
          next.add(path)
        }
        return next
      })
      if (deletesSelectedPath) {
        setIsPreviewOpen(false)
      }
    })
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
        <p className="min-w-0 truncate text-[11px] text-muted-foreground" title={title}>
          {title}
        </p>
        <div className="flex shrink-0 items-center gap-1">
          <input
            ref={fileInputRef}
            type="file"
            className="sr-only"
            onChange={(event) => uploadFile(event.target.files?.[0])}
            aria-label="Upload file to workspace uploads"
          />
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={() => fileInputRef.current?.click()}
            disabled={upload.isPending || !hasGroupId}
            aria-label="Upload file to workspace uploads"
            title="Upload to uploads/"
          >
            <Upload className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={() => void files.refetch()}
            disabled={files.isFetching || !hasGroupId}
            aria-label="Refresh workspace files"
          >
            <RefreshCw className={cn('h-4 w-4', files.isFetching && 'animate-spin')} />
          </Button>
        </div>
      </div>

      {currentPath && (
        <button
          type="button"
          className="flex items-center gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground hover:bg-muted/70 hover:text-foreground"
          onClick={() => setCurrentPath(parentPath(currentPath))}
        >
          <ChevronLeft className="h-3.5 w-3.5" />
          Up one folder
        </button>
      )}

      {selectedCount > 0 && (
        <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground">
          <span>{selectedCount} selected</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 px-2 text-[11px]"
            onClick={insertSelectedPaths}
            disabled={!onInsertPaths}
          >
            Insert paths
          </Button>
        </div>
      )}

      {files.error && (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          {displayError(files.error)}
        </div>
      )}
      {upload.error && (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          {displayError(upload.error)}
        </div>
      )}
      {downloadError && (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          {downloadError}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {!hasGroupId && <p className="p-3 text-sm text-muted-foreground">Select a group to view workspace files.</p>}
        {hasGroupId && files.isLoading && <p className="p-3 text-sm text-muted-foreground">Loading files…</p>}
        {hasGroupId && !files.isLoading && !files.error && sortedFiles.length === 0 && (
          <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground">
            <Folder className="h-8 w-8" />
            <p>No files in this folder.</p>
          </div>
        )}
        {sortedFiles.length > 0 && (
          <ul className="divide-y divide-border">
            {sortedFiles.map((file) => {
              const isSelected = selectedWorkspacePaths.has(file.path)
              return (
                <li
                  key={file.path}
                  draggable
                  className={cn(
                    'group flex items-center gap-2 px-3 py-2 hover:bg-muted/70',
                    isSelected && 'bg-muted ring-1 ring-inset ring-ring/40',
                  )}
                  onDragStart={(event) => handleFileDragStart(event, file)}
                  onContextMenu={(event) => {
                    event.preventDefault()
                    setMenu({ x: event.clientX, y: event.clientY, file })
                  }}
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 text-left"
                    onClick={(event) => handleFileClick(event, file)}
                    aria-pressed={isSelected}
                  >
                    {file.is_dir ? (
                      <Folder className="h-4 w-4 shrink-0 text-primary" />
                    ) : (
                      <File className="h-4 w-4 shrink-0 text-muted-foreground" />
                    )}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">{file.name}</span>
                      <span className="block text-[10px] text-muted-foreground">
                        {file.is_dir ? 'Folder' : formatSize(file.size)}
                      </span>
                    </span>
                  </button>
                  {!file.is_dir && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0 text-muted-foreground opacity-100 sm:opacity-0 sm:group-hover:opacity-100"
                      onClick={() => downloadFile(file)}
                      disabled={downloadingPath === file.path}
                      aria-label={`Download ${file.name}`}
                    >
                      <Download className="h-3.5 w-3.5" />
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 text-muted-foreground opacity-100 sm:opacity-0 sm:group-hover:opacity-100"
                    onClick={() => startRename(file)}
                    aria-label={`Rename ${file.name}`}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 text-muted-foreground opacity-100 hover:text-destructive sm:opacity-0 sm:group-hover:opacity-100"
                    onClick={() => deletePath(file)}
                    disabled={del.isPending}
                    aria-label={`Delete ${file.name}`}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </div>

      <Dialog open={isPreviewOpen && selectedPath !== null} onOpenChange={setIsPreviewOpen}>
        <DialogContent className="max-h-[85vh] max-w-3xl overflow-hidden p-0">
          <DialogHeader className="border-b border-border px-6 py-4 pr-12">
            <DialogTitle className="truncate text-base">
              {selectedPath ?? 'File preview'}
            </DialogTitle>
            <DialogDescription>
              Preview is bounded by the server and may be truncated for large files.
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-40 overflow-y-auto px-6 py-4">
            {selectedPath && preview.isLoading && (
              <p className="text-sm text-muted-foreground">Loading preview…</p>
            )}
            {selectedPath && preview.error && (
              <p className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
                {displayError(preview.error)}
              </p>
            )}
            {preview.data && !preview.data.is_text && (
              <p className="rounded-md border border-border bg-muted/50 p-3 text-sm text-muted-foreground">
                {preview.data.message ?? 'Preview is not available for this file.'}
              </p>
            )}
            {preview.data?.is_text && (
              <pre className="max-h-[60vh] overflow-auto rounded-md bg-muted p-3 text-xs leading-relaxed whitespace-pre-wrap break-words text-foreground">
                {preview.data.content}
                {preview.data.truncated ? '\n… Preview truncated.' : ''}
              </pre>
            )}
          </div>
        </DialogContent>
      </Dialog>

      {renaming && (
        <div className="border-t border-border p-3">
          <label className="mb-1 block text-xs font-medium" htmlFor="workspace-file-rename">
            Rename path
          </label>
          <div className="flex gap-2">
            <Input
              id="workspace-file-rename"
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              className="h-8 text-xs"
            />
            <Button size="sm" onClick={submitRename} disabled={rename.isPending}>
              Save
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setRenaming(null)}>
              Cancel
            </Button>
          </div>
          {rename.error && <p className="mt-2 text-xs text-destructive">{displayError(rename.error)}</p>}
        </div>
      )}

      {menu && (
        <div
          className="fixed z-50 min-w-44 overflow-hidden rounded-md border border-border bg-background py-1 text-sm text-foreground shadow-md"
          style={{ top: menu.y, left: menu.x }}
          onClick={(event) => event.stopPropagation()}
        >
          <button
            type="button"
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
            {menu.file.is_dir ? 'Open folder' : 'Open preview'}
          </button>
          {desktop && (
            <button
              type="button"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              onClick={() => {
                revealEntry(menu.file)
                setMenu(null)
              }}
            >
              <FolderOpen className="h-3.5 w-3.5" />
              Reveal in File Explorer
            </button>
          )}
          {!menu.file.is_dir && (
            <button
              type="button"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
              onClick={() => {
                downloadFile(menu.file)
                setMenu(null)
              }}
            >
              <Download className="h-3.5 w-3.5" />
              Download
            </button>
          )}
          <button
            type="button"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
            onClick={() => {
              startRename(menu.file)
              setMenu(null)
            }}
          >
            <Pencil className="h-3.5 w-3.5" />
            Rename
          </button>
          <button
            type="button"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-destructive hover:bg-muted"
            onClick={() => {
              deletePath(menu.file)
              setMenu(null)
            }}
          >
            <Trash2 className="h-3.5 w-3.5" />
            Delete
          </button>
        </div>
      )}
    </div>
  )
}
