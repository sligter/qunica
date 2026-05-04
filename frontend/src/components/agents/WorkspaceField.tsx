import { useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateWorkspace, useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api'
import { pickFolder, type FolderPickResult } from '@/lib/folderPicker'
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
  const fallbackInputRef = useRef<HTMLInputElement | null>(null)
  const pathInputRef = useRef<HTMLInputElement | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [workspaceName, setWorkspaceName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [pickerHint, setPickerHint] = useState<string | null>(null)
  const [createError, setCreateError] = useState<string | null>(null)

  const selected = (workspaces.data ?? []).find((workspace) => workspace.id === value)
  const selectedBackendType: WorkspaceBackendType = selected?.backend_type ?? 'local'

  const onManualPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    if (!workspaceName) {
      setWorkspaceName(inferWorkspaceName(nextPath))
    }
  }

  const applyPick = (folderName: string) => {
    if (!folderName) return
    setLocalPath(folderName)
    if (!workspaceName) {
      setWorkspaceName(folderName)
    }
    setPickerHint(
      `Picked folder "${folderName}". Browsers cannot expose absolute paths — edit this field to the absolute backend path.`,
    )
    requestAnimationFrame(() => {
      const input = pathInputRef.current
      if (input) {
        input.focus()
        input.setSelectionRange(0, input.value.length)
      }
    })
  }

  const onPickFolder = async () => {
    setCreateError(null)
    setPickerHint(null)
    const result: FolderPickResult = await pickFolder()
    if (result.kind === 'native') {
      applyPick(result.name)
      return
    }
    if (result.kind === 'cancelled') {
      return
    }
    fallbackInputRef.current?.click()
  }

  const onFallbackChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    const relative = file?.webkitRelativePath
    if (relative) {
      const folderName = relative.split('/')[0] ?? ''
      applyPick(folderName)
    }
    if (fallbackInputRef.current) {
      fallbackInputRef.current.value = ''
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
      setPickerHint(null)
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
            <div className="flex gap-2">
              <Input
                id="workspace-path"
                ref={pathInputRef}
                value={localPath}
                onChange={(event) => onManualPathChange(event.target.value)}
                placeholder="D:/absolute/path/to/project or /absolute/path/to/project"
                className={cn(localPath && !localPathLooksAbsolute(localPath) && 'border-red-500')}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void onPickFolder()}
              >
                Pick folder
              </Button>
            </div>
            <p className="text-[11px] text-muted-foreground">
              Enter an absolute path on the machine running the backend. Nothing
              is uploaded; the backend reads/writes this directory directly. The
              picker only fills in the folder name as a starting point.
            </p>
            {pickerHint ? (
              <p className="text-[11px] text-muted-foreground">{pickerHint}</p>
            ) : null}
            <input
              ref={fallbackInputRef}
              type="file"
              className="hidden"
              multiple
              {...({
                webkitdirectory: '',
                directory: '',
              } as Record<string, string>)}
              onChange={onFallbackChange}
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
