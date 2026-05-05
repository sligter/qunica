import { useMemo, useState } from 'react'
import { ChevronLeft, File, Folder, Pencil, RefreshCw, Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  useDeleteGroupWorkspaceFile,
  useGroupWorkspaceFilePreview,
  useGroupWorkspaceFiles,
  useRenameGroupWorkspaceFile,
} from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import type { GroupWorkspaceFileRead } from '@/types/api'

interface GroupWorkspaceFilesPanelProps {
  groupId: string | undefined
  className?: string
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

export function GroupWorkspaceFilesPanel({ groupId, className }: GroupWorkspaceFilesPanelProps) {
  const [currentPath, setCurrentPath] = useState('')
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<GroupWorkspaceFileRead | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const files = useGroupWorkspaceFiles(groupId, currentPath)
  const preview = useGroupWorkspaceFilePreview(groupId, selectedPath)
  const rename = useRenameGroupWorkspaceFile(groupId)
  const del = useDeleteGroupWorkspaceFile(groupId)
  const hasGroupId = groupId !== undefined && groupId.length > 0

  const title = currentPath || 'Workspace root'
  const sortedFiles = useMemo(() => files.data ?? [], [files.data])

  const startRename = (file: GroupWorkspaceFileRead) => {
    setRenaming(file)
    setRenameValue(file.path)
  }

  const submitRename = () => {
    if (!hasGroupId || !renaming || !renameValue.trim()) return
    const oldPath = renaming.path
    void rename
      .mutateAsync({ path: oldPath, newPath: renameValue.trim() })
      .then((next) => {
        setRenaming(null)
        setSelectedPath((path) => (path === oldPath ? next.path : path))
        if (parentPath(oldPath) !== parentPath(next.path)) {
          setCurrentPath(parentPath(next.path))
        }
      })
  }

  const deletePath = (path: string) => {
    if (!hasGroupId) return
    const confirmed = window.confirm(`Delete ${path}?`)
    if (!confirmed) return
    void del.mutateAsync(path).then(() => {
      setSelectedPath((selected) => (selected === path ? null : selected))
    })
  }

  return (
    <aside className={cn('flex h-full w-80 shrink-0 flex-col border-l border-border bg-card', className)}>
      <div className="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-border px-3">
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold">Workspace files</h2>
          <p className="truncate text-[11px] text-muted-foreground" title={title}>{title}</p>
        </div>
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

      {files.error && (
        <div className="m-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          {displayError(files.error)}
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
            {sortedFiles.map((file) => (
              <li key={file.path} className="group flex items-center gap-2 px-3 py-2 hover:bg-muted/70">
                <button
                  type="button"
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                  onClick={() => {
                    if (file.is_dir) {
                      setCurrentPath(file.path)
                      setSelectedPath(null)
                    } else {
                      setSelectedPath(file.path)
                    }
                  }}
                >
                  {file.is_dir ? (
                    <Folder className="h-4 w-4 shrink-0 text-blue-500" />
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
                  onClick={() => deletePath(file.path)}
                  disabled={del.isPending}
                  aria-label={`Delete ${file.name}`}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="max-h-[45%] shrink-0 border-t border-border p-3">
        <h3 className="mb-2 truncate text-xs font-semibold text-muted-foreground">
          {selectedPath ? `Preview: ${selectedPath}` : 'Preview'}
        </h3>
        {!selectedPath && <p className="text-xs text-muted-foreground">Select a text-like file to preview it.</p>}
        {selectedPath && preview.isLoading && <p className="text-xs text-muted-foreground">Loading preview…</p>}
        {selectedPath && preview.error && (
          <p className="text-xs text-destructive">{displayError(preview.error)}</p>
        )}
        {preview.data && !preview.data.is_text && (
          <p className="text-xs text-muted-foreground">{preview.data.message ?? 'Preview is not available.'}</p>
        )}
        {preview.data?.is_text && (
          <pre className="max-h-56 overflow-auto rounded-md bg-muted p-2 text-[11px] leading-relaxed text-foreground whitespace-pre-wrap">
            {preview.data.content}
            {preview.data.truncated ? '\n… Preview truncated.' : ''}
          </pre>
        )}
      </div>

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
    </aside>
  )
}
