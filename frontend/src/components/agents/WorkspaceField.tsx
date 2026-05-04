import { useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateWorkspace, useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api'
import type { WorkspaceBackendType } from '@/types/api'

interface WorkspaceFieldProps {
  value: string
  onChange: (workspaceId: string) => void
  error?: string
}

function inferWorkspaceName(path: string) {
  const normalized = path.replace(/\\/g, '/').replace(/\/+$/, '')
  const last = normalized.split('/').filter(Boolean).pop()
  return last ?? ''
}

function directoryLabel(files: FileList | null) {
  const first = files?.[0]
  const relativePath = first?.webkitRelativePath
  if (!relativePath) return ''
  return relativePath.split('/')[0] ?? ''
}

export function WorkspaceField({ value, onChange, error }: WorkspaceFieldProps) {
  const workspaces = useWorkspaces()
  const createWorkspace = useCreateWorkspace()
  const directoryInputRef = useRef<HTMLInputElement | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [workspaceName, setWorkspaceName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [pickedFolderLabel, setPickedFolderLabel] = useState('')
  const [createError, setCreateError] = useState<string | null>(null)

  const selected = (workspaces.data ?? []).find((workspace) => workspace.id === value)
  const selectedBackendType: WorkspaceBackendType = selected?.backend_type ?? 'local'

  const onManualPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    if (!workspaceName) {
      setWorkspaceName(inferWorkspaceName(nextPath))
    }
  }

  const onDirectoryPicked = (files: FileList | null) => {
    const label = directoryLabel(files)
    if (!label) return
    setPickedFolderLabel(label)
    if (!workspaceName) {
      setWorkspaceName(label)
    }
  }

  const onCreate = async () => {
    setCreateError(null)
    try {
      const created = await createWorkspace.mutateAsync({
        name: workspaceName,
        backend_type: 'local',
        local_path: localPath,
      })
      onChange(created.id)
      setWorkspaceName('')
      setLocalPath('')
      setPickedFolderLabel('')
      setShowCreate(false)
    } catch (err) {
      setCreateError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  return (
    <div className="space-y-2">
      <div className="space-y-1.5">
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="agent-workspace">Workspace</Label>
          <Button type="button" variant="outline" size="sm" onClick={() => setShowCreate(!showCreate)}>
            {showCreate ? 'Cancel' : 'New local workspace'}
          </Button>
        </div>
        <select
          id="agent-workspace"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <option value="">Select workspace</option>
          {(workspaces.data ?? []).map((workspace) => (
            <option key={workspace.id} value={workspace.id}>
              {workspace.name} — {workspace.backend_type}
            </option>
          ))}
        </select>
        {error && <p className="text-xs text-red-600">{error}</p>}
        {workspaces.data && workspaces.data.length === 0 && !showCreate && (
          <p className="text-[11px] text-muted-foreground">
            Create a local workspace first. Cloud sandbox workspaces can use the same field later.
          </p>
        )}
        {selected && (
          <p className="text-[11px] text-muted-foreground">
            Bound to {selectedBackendType}: {selected.local_path ?? selected.sandbox_ref ?? 'not configured'}
          </p>
        )}
      </div>

      {showCreate && (
        <div className="space-y-3 rounded-md border border-border bg-card p-3">
          <div className="space-y-1.5">
            <Label htmlFor="workspace-name">Workspace name</Label>
            <Input
              id="workspace-name"
              value={workspaceName}
              onChange={(event) => setWorkspaceName(event.target.value)}
              placeholder="Current project"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="workspace-path">Backend local path</Label>
            <Input
              id="workspace-path"
              value={localPath}
              onChange={(event) => onManualPathChange(event.target.value)}
              placeholder="/absolute/path/to/project"
            />
            <p className="text-[11px] text-muted-foreground">
              Enter a path the backend process can access. Browser folder picking cannot reliably expose backend-accessible absolute OS paths, and no files are uploaded.
            </p>
          </div>
          <div className="space-y-1.5">
            <Label>Browser folder hint</Label>
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => directoryInputRef.current?.click()}
              >
                Pick folder label
              </Button>
              <span className="text-xs text-muted-foreground">
                {pickedFolderLabel ? `Selected label: ${pickedFolderLabel}` : 'Optional: helps infer a friendly name only.'}
              </span>
            </div>
            <input
              ref={directoryInputRef}
              type="file"
              className="hidden"
              multiple
              // @ts-expect-error Browser-specific directory picker affordances.
              webkitdirectory=""
              directory=""
              onChange={(event) => onDirectoryPicked(event.target.files)}
            />
          </div>
          {createError && <p className="text-xs text-red-600">{createError}</p>}
          <Button
            type="button"
            size="sm"
            disabled={createWorkspace.isPending || !workspaceName || !localPath}
            onClick={() => void onCreate()}
          >
            {createWorkspace.isPending ? 'Creating…' : 'Create workspace'}
          </Button>
        </div>
      )}
    </div>
  )
}
