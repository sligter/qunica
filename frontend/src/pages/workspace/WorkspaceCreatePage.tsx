import { useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateWorkspace } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import {
  basename,
  composePickedPath,
  looksAbsolute,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
} from '@/lib/folderPicker'

const PICKER_SCOPE = 'workspace-management-root'

export function WorkspaceCreatePage() {
  const navigate = useNavigate()
  const createWorkspace = useCreateWorkspace()
  const [name, setName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [error, setError] = useState<string | null>(null)

  const trimmedName = name.trim()
  const trimmedPath = localPath.trim()
  const canCreate =
    trimmedName.length > 0 &&
    trimmedPath.length > 0 &&
    looksAbsolute(trimmedPath) &&
    !createWorkspace.isPending

  const onPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
    if (!name.trim()) {
      setName(basename(nextPath.trim()) || '')
    }
  }

  const onPickFolder = async () => {
    setError(null)
    const result = await pickFolder()
    if (result.kind === 'native') {
      const nextPath =
        result.path ??
        composePickedPath(localPath, result.name, readRememberedPrefix(PICKER_SCOPE))
      setLocalPath(nextPath)
      saveRememberedPrefix(PICKER_SCOPE, nextPath)
      if (!name.trim()) {
        setName(result.name)
      }
      return
    }
    if (result.kind === 'cancelled') return
    if (result.kind === 'fallback') {
      setError('Folder picker is unavailable here. Enter an absolute backend path.')
      return
    }
    setError(result.message)
  }

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!looksAbsolute(trimmedPath)) {
      setError('Enter an absolute backend path.')
      return
    }
    setError(null)
    createWorkspace.mutate(
      {
        name: trimmedName,
        backend_type: 'local',
        local_path: trimmedPath,
      },
      {
        onSuccess: (created) => {
          void navigate(`/workspaces/${created.id}`)
        },
        onError: (err) => {
          setError(err instanceof ApiError ? err.message : 'Failed to create workspace')
        },
      },
    )
  }

  return (
    <DetailShell
      title="New local workspace"
      subtitle="A workspace points at an absolute folder on the backend host. Groups and agents bound to it read and write files there."
    >
      <form onSubmit={onSubmit} className="space-y-4">
        <div className="space-y-1.5">
          <Label htmlFor="workspace-new-name">Name</Label>
          <Input
            id="workspace-new-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Current project"
            className="max-w-xl"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="workspace-new-path">Backend local path</Label>
          <div className="flex max-w-xl gap-2">
            <Input
              id="workspace-new-path"
              value={localPath}
              onChange={(event) => onPathChange(event.target.value)}
              placeholder="D:/absolute/path/to/project"
              className={
                trimmedPath && !looksAbsolute(trimmedPath)
                  ? 'border-destructive'
                  : undefined
              }
            />
            <Button type="button" variant="outline" onClick={() => void onPickFolder()}>
              Pick folder
            </Button>
          </div>
          {trimmedPath && !looksAbsolute(trimmedPath) ? (
            <p className="text-xs text-destructive">
              Local workspace paths must be absolute.
            </p>
          ) : null}
        </div>
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
        <Button type="submit" disabled={!canCreate}>
          {createWorkspace.isPending ? 'Creating…' : 'Create workspace'}
        </Button>
      </form>
    </DetailShell>
  )
}
