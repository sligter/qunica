import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateWorkspace, useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
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

function localPathLooksAbsolute(path: string) {
  const trimmed = path.trim()
  return /^(?:[a-zA-Z]:[\\/]|\\\\|\/)/.test(trimmed)
}

export function WorkspaceField({ value, onChange, error }: WorkspaceFieldProps) {
  const workspaces = useWorkspaces()
  const createWorkspace = useCreateWorkspace()
  const [showCreate, setShowCreate] = useState(false)
  const [workspaceName, setWorkspaceName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [createError, setCreateError] = useState<string | null>(null)

  const selected = (workspaces.data ?? []).find((workspace) => workspace.id === value)
  const selectedBackendType: WorkspaceBackendType = selected?.backend_type ?? 'local'

  const onManualPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    if (!workspaceName) {
      setWorkspaceName(inferWorkspaceName(nextPath))
    }
  }

  const onCreate = async () => {
    if (!localPathLooksAbsolute(localPath)) {
      setCreateError(
        'Enter an absolute path that exists on the backend host, for example D:/file/learn/AIGC/ag-swarmer or /home/me/project.',
      )
      return
    }
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
              placeholder="D:/absolute/path/to/project or /absolute/path/to/project"
              className={cn(localPath && !localPathLooksAbsolute(localPath) && 'border-red-500')}
            />
            <p className="text-[11px] text-muted-foreground">
              Enter an absolute path on the machine running the backend. Nothing
              is uploaded; the backend reads/writes this directory directly.
            </p>
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
