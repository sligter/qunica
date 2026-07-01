import { useEffect, useMemo, useState } from 'react'
import {
  Bot,
  Folder,
  HardDrive,
  Link2,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  Users,
} from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  useSetGroupAgentWorkspaceSharing,
} from '@/hooks/useGroupAgentActions'
import { useAgents } from '@/hooks/useAgents'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useGroups, useUpdateGroup } from '@/hooks/useGroups'
import { useUpdateAgent } from '@/hooks/useUpdateAgent'
import {
  useCreateWorkspace,
  useDeleteWorkspace,
  useUpdateWorkspace,
  useWorkspaces,
} from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/http'
import {
  basename,
  composePickedPath,
  looksAbsolute,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
} from '@/lib/folderPicker'
import type {
  AgentRead,
  GroupAgentRead,
  GroupRead,
  WorkspaceRead,
  WorkspaceUpdate,
} from '@/types/api'

const PICKER_SCOPE = 'workspace-management-root'
const EMPTY_WORKSPACES: WorkspaceRead[] = []
const EMPTY_GROUPS: GroupRead[] = []
const EMPTY_AGENTS: AgentRead[] = []

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof ApiError ? error.message : fallback
}

function workspaceLocation(workspace: WorkspaceRead): string {
  if (workspace.backend_type === 'local') {
    return workspace.local_path ?? 'No local path'
  }
  return workspace.sandbox_ref ?? 'No sandbox reference'
}

function workspaceName(
  workspaceById: Map<string, WorkspaceRead>,
  workspaceId: string | null,
): string {
  if (!workspaceId) return 'No workspace'
  return workspaceById.get(workspaceId)?.name ?? 'Unknown workspace'
}

function inferWorkspaceName(path: string): string {
  return basename(path.trim()) || ''
}

interface WorkspaceSelectProps {
  value: string
  workspaces: WorkspaceRead[]
  disabled?: boolean
  label: string
  onChange: (workspaceId: string) => void
}

function WorkspaceSelect({
  value,
  workspaces,
  disabled = false,
  label,
  onChange,
}: WorkspaceSelectProps) {
  const hasCurrentWorkspace =
    value === '' || workspaces.some((workspace) => workspace.id === value)

  return (
    <select
      value={value}
      disabled={disabled || workspaces.length === 0}
      aria-label={label}
      onChange={(event) => onChange(event.target.value)}
      className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60 sm:w-64"
    >
      <option value="" disabled>
        {workspaces.length === 0 ? 'No workspaces available' : 'Select workspace'}
      </option>
      {!hasCurrentWorkspace ? (
        <option value={value}>Unknown workspace</option>
      ) : null}
      {workspaces.map((workspace) => (
        <option key={workspace.id} value={workspace.id}>
          {workspace.name}
        </option>
      ))}
    </select>
  )
}

function CreateWorkspacePanel() {
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
      setName(inferWorkspaceName(nextPath))
    }
  }

  const applyPickedFolder = (folderName: string, absolutePath?: string) => {
    const nextPath =
      absolutePath ??
      composePickedPath(localPath, folderName, readRememberedPrefix(PICKER_SCOPE))
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
    if (!name.trim()) {
      setName(folderName)
    }
  }

  const onPickFolder = async () => {
    setError(null)
    const result = await pickFolder()
    if (result.kind === 'native') {
      applyPickedFolder(result.name, result.path)
      return
    }
    if (result.kind === 'cancelled') return
    if (result.kind === 'fallback') {
      setError('Folder picker is unavailable here. Enter an absolute backend path.')
      return
    }
    setError(result.message)
  }

  const onCreate = () => {
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
        onSuccess: () => {
          setName('')
          setLocalPath('')
        },
        onError: (err) => {
          setError(errorMessage(err, 'Failed to create workspace'))
        },
      },
    )
  }

  return (
    <section className="rounded-lg border border-border bg-card p-4">
      <div className="mb-4 flex items-center gap-2">
        <Plus className="h-4 w-4 text-muted-foreground" />
        <h2 className="text-sm font-semibold">New local workspace</h2>
      </div>
      <div className="grid gap-3 lg:grid-cols-[240px_1fr_auto]">
        <div className="space-y-1.5">
          <Label htmlFor="workspace-new-name">Name</Label>
          <Input
            id="workspace-new-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Current project"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="workspace-new-path">Backend local path</Label>
          <div className="flex gap-2">
            <Input
              id="workspace-new-path"
              value={localPath}
              onChange={(event) => onPathChange(event.target.value)}
              placeholder="D:/absolute/path/to/project"
              className={
                trimmedPath && !looksAbsolute(trimmedPath) ? 'border-red-500' : undefined
              }
            />
            <Button type="button" variant="outline" onClick={() => void onPickFolder()}>
              Pick folder
            </Button>
          </div>
        </div>
        <div className="flex items-end">
          <Button type="button" onClick={onCreate} disabled={!canCreate}>
            {createWorkspace.isPending ? 'Creating' : 'Create'}
          </Button>
        </div>
      </div>
      {error ? <p className="mt-3 text-xs text-red-600">{error}</p> : null}
    </section>
  )
}

interface WorkspaceCardProps {
  workspace: WorkspaceRead
}

function WorkspaceCard({ workspace }: WorkspaceCardProps) {
  const updateWorkspace = useUpdateWorkspace(workspace.id)
  const deleteWorkspace = useDeleteWorkspace()
  const [name, setName] = useState(workspace.name)
  const [localPath, setLocalPath] = useState(workspace.local_path ?? '')
  const [sandboxRef, setSandboxRef] = useState(workspace.sandbox_ref ?? '')
  const [error, setError] = useState<string | null>(null)

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

  const onDelete = () => {
    if (
      !confirm(
        `Delete workspace "${workspace.name}"? This hides it from the workspace list and clears it from active groups and agents that currently use it.`,
      )
    ) {
      return
    }
    setError(null)
    deleteWorkspace.mutate(workspace.id, {
      onError: (err) => {
        setError(errorMessage(err, 'Failed to delete workspace'))
      },
    })
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0 pb-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate text-sm font-semibold">{workspace.name}</h3>
            <Badge variant="outline">{workspace.backend_type}</Badge>
            <Badge variant={workspace.status === 'active' ? 'default' : 'secondary'}>
              {workspace.status}
            </Badge>
          </div>
          <p className="mt-1 truncate text-xs text-muted-foreground">
            {workspaceLocation(workspace)}
          </p>
        </div>
        <HardDrive className="h-5 w-5 shrink-0 text-muted-foreground" />
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid gap-3 lg:grid-cols-[220px_1fr_auto]">
          <div className="space-y-1.5">
            <Label htmlFor={`workspace-name-${workspace.id}`}>Rename</Label>
            <Input
              id={`workspace-name-${workspace.id}`}
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </div>
          {workspace.backend_type === 'local' ? (
            <div className="space-y-1.5">
              <Label htmlFor={`workspace-local-${workspace.id}`}>Local path</Label>
              <div className="flex gap-2">
                <Input
                  id={`workspace-local-${workspace.id}`}
                  value={localPath}
                  onChange={(event) => onPathChange(event.target.value)}
                  className={localPathInvalid ? 'border-red-500' : undefined}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void onPickFolder()}
                >
                  Pick folder
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-1.5">
              <Label htmlFor={`workspace-sandbox-${workspace.id}`}>Sandbox ref</Label>
              <Input
                id={`workspace-sandbox-${workspace.id}`}
                value={sandboxRef}
                onChange={(event) => setSandboxRef(event.target.value)}
              />
            </div>
          )}
          <div className="flex items-end">
            <Button type="button" onClick={onSave} disabled={!canSave}>
              <Save className="h-4 w-4" />
              {updateWorkspace.isPending ? 'Renaming' : 'Rename'}
            </Button>
          </div>
        </div>
        <div className="flex justify-end border-t border-border pt-3">
          <Button
            type="button"
            variant="destructive"
            size="sm"
            onClick={onDelete}
            disabled={updateWorkspace.isPending || deleteWorkspace.isPending}
          >
            <Trash2 className="h-4 w-4" />
            {deleteWorkspace.isPending ? 'Deleting' : 'Delete workspace'}
          </Button>
        </div>
        {localPathInvalid ? (
          <p className="text-xs text-red-600">Local workspace paths must be absolute.</p>
        ) : null}
        {error ? <p className="text-xs text-red-600">{error}</p> : null}
      </CardContent>
    </Card>
  )
}

interface GroupBindingRowProps {
  group: GroupRead
  workspaces: WorkspaceRead[]
  workspaceById: Map<string, WorkspaceRead>
}

function GroupBindingRow({
  group,
  workspaces,
  workspaceById,
}: GroupBindingRowProps) {
  const updateGroup = useUpdateGroup(group.id)
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(group.workspace_id ?? '')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setSelectedWorkspaceId(group.workspace_id ?? '')
  }, [group.workspace_id])

  const onWorkspaceChange = (workspaceId: string) => {
    if (!workspaceId || workspaceId === group.workspace_id) return
    setSelectedWorkspaceId(workspaceId)
    setError(null)
    updateGroup.mutate(
      { workspace_id: workspaceId },
      {
        onError: (err) => {
          setSelectedWorkspaceId(group.workspace_id ?? '')
          setError(errorMessage(err, 'Failed to update group workspace'))
        },
      },
    )
  }

  return (
    <li className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Users className="h-4 w-4 text-muted-foreground" />
            <p className="truncate text-sm font-medium">{group.name}</p>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            Current: {workspaceName(workspaceById, group.workspace_id)}
          </p>
        </div>
        <WorkspaceSelect
          value={selectedWorkspaceId}
          workspaces={workspaces}
          disabled={updateGroup.isPending}
          label={`Workspace for group ${group.name}`}
          onChange={onWorkspaceChange}
        />
      </div>
      {error ? <p className="mt-2 text-xs text-red-600">{error}</p> : null}
    </li>
  )
}

interface AgentBindingRowProps {
  agent: AgentRead
  workspaces: WorkspaceRead[]
  workspaceById: Map<string, WorkspaceRead>
}

function AgentBindingRow({
  agent,
  workspaces,
  workspaceById,
}: AgentBindingRowProps) {
  const updateAgent = useUpdateAgent(agent.id)
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(agent.workspace_id ?? '')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setSelectedWorkspaceId(agent.workspace_id ?? '')
  }, [agent.workspace_id])

  const onWorkspaceChange = (workspaceId: string) => {
    if (!workspaceId || workspaceId === agent.workspace_id) return
    setSelectedWorkspaceId(workspaceId)
    setError(null)
    updateAgent.mutate(
      { workspace_id: workspaceId },
      {
        onError: (err) => {
          setSelectedWorkspaceId(agent.workspace_id ?? '')
          setError(errorMessage(err, 'Failed to update agent workspace'))
        },
      },
    )
  }

  return (
    <li className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Bot className="h-4 w-4 text-muted-foreground" />
            <p className="truncate text-sm font-medium">{agent.name}</p>
            <Badge variant="outline">{agent.runtime_kind}</Badge>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            Current: {workspaceName(workspaceById, agent.workspace_id)}
          </p>
        </div>
        <WorkspaceSelect
          value={selectedWorkspaceId}
          workspaces={workspaces}
          disabled={updateAgent.isPending}
          label={`Workspace for agent ${agent.name}`}
          onChange={onWorkspaceChange}
        />
      </div>
      {error ? <p className="mt-2 text-xs text-red-600">{error}</p> : null}
    </li>
  )
}

interface GroupAgentSharingRowProps {
  membership: GroupAgentRead
  group: GroupRead
  agent: AgentRead | undefined
  workspaceById: Map<string, WorkspaceRead>
}

function GroupAgentSharingRow({
  membership,
  group,
  agent,
  workspaceById,
}: GroupAgentSharingRowProps) {
  const setSharing = useSetGroupAgentWorkspaceSharing()
  const [error, setError] = useState<string | null>(null)
  const groupWorkspaceName = workspaceName(workspaceById, group.workspace_id)
  const agentWorkspaceName = workspaceName(workspaceById, agent?.workspace_id ?? null)

  const onToggleSharing = () => {
    setError(null)
    setSharing.mutate(
      {
        groupId: group.id,
        agentId: membership.agent_id,
        shareGroupWorkspace: !membership.share_group_workspace,
      },
      {
        onError: (err) => {
          setError(errorMessage(err, 'Failed to update workspace sharing'))
        },
      },
    )
  }

  return (
    <li className="rounded-md border border-border px-3 py-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <p className="truncate text-sm font-medium">
              {membership.display_name || agent?.name || membership.agent_id}
            </p>
            <Badge variant={membership.share_group_workspace ? 'default' : 'secondary'}>
              {membership.share_group_workspace ? 'Group workspace' : 'Agent workspace'}
            </Badge>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            Uses {membership.share_group_workspace ? groupWorkspaceName : agentWorkspaceName}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onToggleSharing}
          disabled={setSharing.isPending}
        >
          {membership.share_group_workspace ? 'Use agent workspace' : 'Use group workspace'}
        </Button>
      </div>
      {error ? <p className="mt-2 text-xs text-red-600">{error}</p> : null}
    </li>
  )
}

interface GroupSharingCardProps {
  group: GroupRead
  agentsById: Map<string, AgentRead>
  workspaceById: Map<string, WorkspaceRead>
}

function GroupSharingCard({
  group,
  agentsById,
  workspaceById,
}: GroupSharingCardProps) {
  const groupAgents = useGroupAgents(group.id)

  return (
    <section className="rounded-lg border border-border bg-card p-4">
      <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Link2 className="h-4 w-4 text-muted-foreground" />
            <h3 className="truncate text-sm font-semibold">{group.name}</h3>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            Group workspace: {workspaceName(workspaceById, group.workspace_id)}
          </p>
        </div>
        <Badge variant="outline">{group.communication_mode}</Badge>
      </div>
      {groupAgents.isLoading ? (
        <p className="text-sm text-muted-foreground">Loading agents...</p>
      ) : null}
      {groupAgents.error ? (
        <p className="text-sm text-red-600">Failed to load group agents.</p>
      ) : null}
      {groupAgents.data && groupAgents.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">No agents in this group.</p>
      ) : null}
      {groupAgents.data && groupAgents.data.length > 0 ? (
        <ul className="space-y-2">
          {groupAgents.data.map((membership) => (
            <GroupAgentSharingRow
              key={membership.agent_id}
              membership={membership}
              group={group}
              agent={agentsById.get(membership.agent_id)}
              workspaceById={workspaceById}
            />
          ))}
        </ul>
      ) : null}
    </section>
  )
}

interface BindingsTabProps {
  groups: GroupRead[]
  agents: AgentRead[]
  workspaces: WorkspaceRead[]
  workspaceById: Map<string, WorkspaceRead>
}

function BindingsTab({
  groups,
  agents,
  workspaces,
  workspaceById,
}: BindingsTabProps) {
  return (
    <div className="grid gap-6 xl:grid-cols-2">
      <section className="space-y-3">
        <div>
          <h2 className="text-sm font-semibold">Groups</h2>
          <p className="text-xs text-muted-foreground">
            {groups.length} group{groups.length === 1 ? '' : 's'}
          </p>
        </div>
        {groups.length === 0 ? (
          <p className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
            No groups yet.
          </p>
        ) : (
          <ul className="space-y-3">
            {groups.map((group) => (
              <GroupBindingRow
                key={group.id}
                group={group}
                workspaces={workspaces}
                workspaceById={workspaceById}
              />
            ))}
          </ul>
        )}
      </section>

      <section className="space-y-3">
        <div>
          <h2 className="text-sm font-semibold">Agents</h2>
          <p className="text-xs text-muted-foreground">
            {agents.length} agent{agents.length === 1 ? '' : 's'}
          </p>
        </div>
        {agents.length === 0 ? (
          <p className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
            No agents yet.
          </p>
        ) : (
          <ul className="space-y-3">
            {agents.map((agent) => (
              <AgentBindingRow
                key={agent.id}
                agent={agent}
                workspaces={workspaces}
                workspaceById={workspaceById}
              />
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}

interface SharingTabProps {
  groups: GroupRead[]
  agentsById: Map<string, AgentRead>
  workspaceById: Map<string, WorkspaceRead>
}

function SharingTab({ groups, agentsById, workspaceById }: SharingTabProps) {
  if (groups.length === 0) {
    return (
      <p className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
        No groups yet.
      </p>
    )
  }

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      {groups.map((group) => (
        <GroupSharingCard
          key={group.id}
          group={group}
          agentsById={agentsById}
          workspaceById={workspaceById}
        />
      ))}
    </div>
  )
}

export function WorkspacesPage() {
  const workspaces = useWorkspaces()
  const groups = useGroups()
  const agents = useAgents()

  const workspaceList = workspaces.data ?? EMPTY_WORKSPACES
  const groupList = groups.data ?? EMPTY_GROUPS
  const agentList = agents.data ?? EMPTY_AGENTS

  const workspaceById = useMemo(
    () => new Map(workspaceList.map((workspace) => [workspace.id, workspace])),
    [workspaceList],
  )
  const agentsById = useMemo(
    () => new Map(agentList.map((agent) => [agent.id, agent])),
    [agentList],
  )

  const loadError = workspaces.error ?? groups.error ?? agents.error
  const isLoading = workspaces.isLoading || groups.isLoading || agents.isLoading

  const onRefresh = () => {
    void workspaces.refetch()
    void groups.refetch()
    void agents.refetch()
  }

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-6">
        <div className="flex items-center gap-2">
          <Folder className="h-5 w-5 text-muted-foreground" />
          <h1 className="text-base font-semibold tracking-tight">Workspace</h1>
          <span className="text-xs text-muted-foreground">({workspaceList.length})</span>
        </div>
        <Button type="button" size="sm" variant="outline" onClick={onRefresh}>
          <RefreshCw className="h-4 w-4" />
          Refresh
        </Button>
      </header>

      <Tabs defaultValue="workspaces" className="flex min-h-0 flex-1 flex-col">
        <div className="shrink-0 border-b border-border px-6 py-3">
          <TabsList>
            <TabsTrigger value="workspaces">Workspaces</TabsTrigger>
            <TabsTrigger value="bindings">Bindings</TabsTrigger>
            <TabsTrigger value="sharing">Sharing</TabsTrigger>
          </TabsList>
        </div>

        {loadError ? (
          <div className="border-b border-border px-6 py-3 text-sm text-red-600">
            Failed to load workspace data: {errorMessage(loadError, 'Network error')}
          </div>
        ) : null}
        {isLoading ? (
          <div className="border-b border-border px-6 py-3 text-sm text-muted-foreground">
            Loading workspace data...
          </div>
        ) : null}

        <TabsContent value="workspaces" className="min-h-0 flex-1 overflow-y-auto p-6">
          <div className="mx-auto max-w-6xl space-y-5">
            <CreateWorkspacePanel />
            {workspaceList.length === 0 ? (
              <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-border py-16 text-center">
                <div className="flex h-12 w-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
                  <HardDrive className="h-6 w-6" />
                </div>
                <h2 className="text-sm font-medium">No workspaces yet</h2>
              </div>
            ) : (
              <div className="grid gap-4">
                {workspaceList.map((workspace) => (
                  <WorkspaceCard key={workspace.id} workspace={workspace} />
                ))}
              </div>
            )}
          </div>
        </TabsContent>

        <TabsContent value="bindings" className="min-h-0 flex-1 overflow-y-auto p-6">
          <div className="mx-auto max-w-6xl">
            <BindingsTab
              groups={groupList}
              agents={agentList}
              workspaces={workspaceList}
              workspaceById={workspaceById}
            />
          </div>
        </TabsContent>

        <TabsContent value="sharing" className="min-h-0 flex-1 overflow-y-auto p-6">
          <div className="mx-auto max-w-6xl">
            <SharingTab
              groups={groupList}
              agentsById={agentsById}
              workspaceById={workspaceById}
            />
          </div>
        </TabsContent>
      </Tabs>
    </div>
  )
}
