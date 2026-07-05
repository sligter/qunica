import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { Bot, Users } from 'lucide-react'

import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { useAgents } from '@/hooks/useAgents'
import { useGroups } from '@/hooks/useGroups'
import {
  useDeleteWorkspace,
  useUpdateWorkspace,
  useWorkspaces,
} from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import {
  composePickedPath,
  looksAbsolute,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
} from '@/lib/folderPicker'
import type { WorkspaceRead, WorkspaceUpdate } from '@/types/api'

const PICKER_SCOPE = 'workspace-management-root'

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof ApiError ? error.message : fallback
}

export function WorkspaceDetailPage() {
  const { workspaceId } = useParams<{ workspaceId: string }>()
  const workspaces = useWorkspaces()
  const navigate = useNavigate()

  if (workspaces.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading workspace…</div>
  }
  if (workspaces.error) {
    return (
      <div className="p-6 text-sm text-destructive">
        Failed to load workspaces: {String(workspaces.error)}
      </div>
    )
  }

  const workspace = (workspaces.data ?? []).find((w) => w.id === workspaceId)
  if (!workspace) {
    return <div className="p-6 text-sm text-muted-foreground">Workspace not found.</div>
  }

  return (
    <WorkspaceDetail
      key={workspace.id}
      workspace={workspace}
      onDeleted={() => void navigate('/settings/workspaces')}
    />
  )
}

interface WorkspaceDetailProps {
  workspace: WorkspaceRead
  onDeleted: () => void
}

function WorkspaceDetail({ workspace, onDeleted }: WorkspaceDetailProps) {
  const updateWorkspace = useUpdateWorkspace(workspace.id)
  const deleteWorkspace = useDeleteWorkspace()
  const [name, setName] = useState(workspace.name)
  const [localPath, setLocalPath] = useState(workspace.local_path ?? '')
  const [sandboxRef, setSandboxRef] = useState(workspace.sandbox_ref ?? '')
  const [error, setError] = useState<string | null>(null)
  const [confirmOpen, setConfirmOpen] = useState(false)

  useEffect(() => {
    setName(workspace.name)
    setLocalPath(workspace.local_path ?? '')
    setSandboxRef(workspace.sandbox_ref ?? '')
  }, [workspace.local_path, workspace.name, workspace.sandbox_ref])

  const trimmedName = name.trim()
  const trimmedLocalPath = localPath.trim()
  const trimmedSandboxRef = sandboxRef.trim()
  const localPathInvalid =
    workspace.backend_type === 'local' &&
    trimmedLocalPath.length > 0 &&
    !looksAbsolute(trimmedLocalPath)
  const dirty =
    trimmedName !== workspace.name ||
    trimmedLocalPath !== (workspace.local_path ?? '') ||
    trimmedSandboxRef !== (workspace.sandbox_ref ?? '')
  const canSave =
    dirty &&
    trimmedName.length > 0 &&
    !localPathInvalid &&
    !updateWorkspace.isPending &&
    !deleteWorkspace.isPending

  const onPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
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
      return
    }
    if (result.kind === 'cancelled') return
    if (result.kind === 'fallback') {
      setError('Folder picker is unavailable here. Enter an absolute backend path.')
      return
    }
    setError(result.message)
  }

  const onSave = () => {
    const payload: WorkspaceUpdate = { name: trimmedName }
    if (workspace.backend_type === 'local') {
      payload.local_path = trimmedLocalPath
    } else {
      payload.sandbox_ref = trimmedSandboxRef || null
    }
    setError(null)
    updateWorkspace.mutate(payload, {
      onError: (err) => {
        setError(errorMessage(err, 'Failed to update workspace'))
      },
    })
  }

  return (
    <DetailShell
      title={workspace.name}
      subtitle={
        <>
          <Badge variant="outline">{workspace.backend_type}</Badge>
          <Badge variant={workspace.status === 'active' ? 'default' : 'secondary'}>
            {workspace.status}
          </Badge>
        </>
      }
      actions={
        <Button
          size="sm"
          variant="destructive"
          onClick={() => setConfirmOpen(true)}
          disabled={updateWorkspace.isPending || deleteWorkspace.isPending}
        >
          {deleteWorkspace.isPending ? 'Deleting…' : 'Delete'}
        </Button>
      }
    >
      <div className="space-y-10">
        <SettingsSection
          title="Workspace"
          aside={
            <Button size="sm" onClick={onSave} disabled={!canSave}>
              {updateWorkspace.isPending ? 'Saving…' : 'Save'}
            </Button>
          }
        >
          <SettingsRow label="Name" htmlFor="workspace-edit-name" stacked>
            <Input
              id="workspace-edit-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="max-w-xl"
            />
          </SettingsRow>
          {workspace.backend_type === 'local' ? (
            <SettingsRow
              label="Backend local path"
              description="Absolute folder on the backend host."
              htmlFor="workspace-edit-path"
              stacked
            >
              <div className="flex max-w-xl gap-2">
                <Input
                  id="workspace-edit-path"
                  value={localPath}
                  onChange={(event) => onPathChange(event.target.value)}
                  className={localPathInvalid ? 'border-destructive' : undefined}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void onPickFolder()}
                >
                  Pick folder
                </Button>
              </div>
              {localPathInvalid ? (
                <p className="text-xs text-destructive">
                  Local workspace paths must be absolute.
                </p>
              ) : null}
            </SettingsRow>
          ) : (
            <SettingsRow label="Sandbox ref" htmlFor="workspace-edit-sandbox" stacked>
              <Input
                id="workspace-edit-sandbox"
                value={sandboxRef}
                onChange={(event) => setSandboxRef(event.target.value)}
                className="max-w-xl"
              />
            </SettingsRow>
          )}
          {error ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {error}
            </p>
          ) : null}
        </SettingsSection>

        <WorkspaceUsageSection workspaceId={workspace.id} />
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={`Delete workspace "${workspace.name}"?`}
        description="This hides it from the workspace list and clears it from active groups and agents that currently use it."
        confirmLabel="Delete"
        destructive
        onConfirm={async () => {
          await deleteWorkspace.mutateAsync(workspace.id)
          onDeleted()
        }}
      />
    </DetailShell>
  )
}

interface WorkspaceUsageSectionProps {
  workspaceId: string
}

/**
 * Read-only view of which groups and agents are bound to this workspace.
 * Bindings are configured on the entity side (agent detail, group manage).
 */
function WorkspaceUsageSection({ workspaceId }: WorkspaceUsageSectionProps) {
  const groups = useGroups()
  const agents = useAgents()

  const boundGroups = (groups.data ?? []).filter((g) => g.workspace_id === workspaceId)
  const boundAgents = (agents.data ?? []).filter((a) => a.workspace_id === workspaceId)
  const isLoading = groups.isLoading || agents.isLoading

  return (
    <SettingsSection
      title="Used by"
      description="Bindings are configured on each group or agent, not here."
    >
      <div className="py-4">
        {isLoading ? (
          <p className="text-sm text-muted-foreground">Loading usage…</p>
        ) : boundGroups.length === 0 && boundAgents.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No groups or agents use this workspace.
          </p>
        ) : (
          <ul className="space-y-1.5">
            {boundGroups.map((group) => (
              <li key={group.id} className="flex items-center gap-2 text-sm">
                <Users className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="truncate">{group.name}</span>
                <Badge variant="outline" className="text-[10px]">
                  group
                </Badge>
              </li>
            ))}
            {boundAgents.map((agent) => (
              <li key={agent.id} className="flex items-center gap-2 text-sm">
                <Bot className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="truncate">{agent.name}</span>
                <Badge variant="outline" className="text-[10px]">
                  agent
                </Badge>
              </li>
            ))}
          </ul>
        )}
      </div>
    </SettingsSection>
  )
}
