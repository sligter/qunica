import { useMemo, useState } from 'react'
import { Bot, Search, UserRound } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { useAddAgentToGroup } from '@/hooks/useAddAgentToGroup'
import { useAgents } from '@/hooks/useAgents'
import { useGroup } from '@/hooks/useGroups'
import {
  useAddGroupMember,
  useGroupMemberCandidates,
  useGroupMembers,
  useMuteGroupMember,
  useRemoveGroupMember,
} from '@/hooks/useGroupMembers'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import {
  useMuteGroupAgent,
  useRemoveGroupAgent,
  useSetGroupAgentTopology,
  useSetGroupAgentWorkspaceSharing,
} from '@/hooks/useGroupAgentActions'
import { ApiError } from '@/lib/api-v2/client'
import type {
  AgentRead,
  GroupAgentRead,
  GroupCommunicationMode,
  GroupMemberRead,
  GroupTopologyRole,
  UserRead,
} from '@/types/api'

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof ApiError ? error.message : fallback
}

function formatRole(role: string | null): string {
  return role ?? 'agent'
}

interface MemberRowProps {
  member: GroupMemberRead
  groupId: string
}

function MemberRow({ member, groupId }: MemberRowProps) {
  const muteMember = useMuteGroupMember()
  const removeMember = useRemoveGroupMember()
  const [error, setError] = useState<string | null>(null)
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false)

  const onMute = () => {
    setError(null)
    muteMember.mutate(
      { groupId, userId: member.user_id, muted: !member.is_muted },
      { onError: (err) => setError(errorMessage(err, 'Failed to update member mute')) },
    )
  }

  const onRemove = () => {
    setError(null)
    removeMember.mutate(
      { groupId, userId: member.user_id },
      { onError: (err) => setError(errorMessage(err, 'Failed to remove member')) },
    )
  }

  return (
    <li className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
            <UserRound className="h-4 w-4" />
          </div>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <p className="truncate text-sm font-medium">{member.display_name}</p>
              <Badge variant="outline">{member.role}</Badge>
              {member.is_muted ? <Badge variant="secondary">Muted</Badge> : null}
            </div>
            <p className="text-xs text-muted-foreground">User ID: {member.user_id}</p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={onMute}
            disabled={muteMember.isPending}
          >
            {member.is_muted ? 'Unmute' : 'Mute'}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => setConfirmRemoveOpen(true)}
            disabled={removeMember.isPending}
            className="text-destructive hover:bg-destructive/10 hover:text-destructive"
          >
            Remove
          </Button>
        </div>
      </div>
      {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
      <ConfirmDialog
        open={confirmRemoveOpen}
        onOpenChange={setConfirmRemoveOpen}
        title={`Remove ${member.display_name}?`}
        description="This member will no longer be part of the group."
        confirmLabel="Remove"
        destructive
        onConfirm={onRemove}
      />
    </li>
  )
}

interface AgentRowProps {
  agent: GroupAgentRead
  groupId: string
  isMuted: boolean
  communicationMode: GroupCommunicationMode
}

function AgentRow({ agent, groupId, isMuted, communicationMode }: AgentRowProps) {
  const muteAgent = useMuteGroupAgent()
  const removeAgent = useRemoveGroupAgent()
  const setSharing = useSetGroupAgentWorkspaceSharing()
  const setTopology = useSetGroupAgentTopology()
  const [error, setError] = useState<string | null>(null)
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false)

  const onMute = () => {
    setError(null)
    muteAgent.mutate(
      { groupId, agentId: agent.agent_id, muted: !isMuted },
      { onError: (err) => setError(errorMessage(err, 'Failed to update agent mute')) },
    )
  }

  const onToggleSharing = () => {
    setError(null)
    setSharing.mutate(
      {
        groupId,
        agentId: agent.agent_id,
        shareGroupWorkspace: !agent.share_group_workspace,
      },
      { onError: (err) => setError(errorMessage(err, 'Failed to update workspace sharing')) },
    )
  }

  const updateTopology = (topologyRole?: GroupTopologyRole | null, speakingOrder?: number | null) => {
    setError(null)
    setTopology.mutate(
      { groupId, agentId: agent.agent_id, topologyRole, speakingOrder },
      { onError: (err) => setError(errorMessage(err, 'Failed to update topology')) },
    )
  }

  const onHierarchyRoleChange = (value: string) => {
    if (value === 'leader' || value === 'worker') {
      updateTopology(value, null)
    }
  }

  const onRingOrderChange = (value: string) => {
    updateTopology(null, value === '' ? null : Number(value))
  }

  const onRemove = () => {
    setError(null)
    removeAgent.mutate(
      { groupId, agentId: agent.agent_id },
      { onError: (err) => setError(errorMessage(err, 'Failed to remove agent')) },
    )
  }

  return (
    <li className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
            <Bot className="h-4 w-4" />
          </div>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <p className="truncate text-sm font-medium">{agent.display_name}</p>
              <Badge variant="outline">{formatRole(agent.role)}</Badge>
              {isMuted ? <Badge variant="secondary">Muted</Badge> : null}
              {communicationMode === 'star' && agent.topology_role === 'hub' ? (
                <Badge variant="secondary">Hub</Badge>
              ) : null}
              {communicationMode === 'hierarchical' ? (
                <Badge variant="secondary">
                  {agent.topology_role === 'leader' ? 'Leader' : 'Worker'}
                </Badge>
              ) : null}
              {communicationMode === 'ring' && agent.speaking_order !== null ? (
                <Badge variant="secondary">Order {agent.speaking_order}</Badge>
              ) : null}
              {agent.share_group_workspace ? <Badge variant="secondary">Group workspace</Badge> : null}
            </div>
            <p className="text-xs text-muted-foreground">Agent ID: {agent.agent_id}</p>
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center gap-2">
          {communicationMode === 'star' ? (
            <Button
              size="sm"
              variant={agent.topology_role === 'hub' ? 'default' : 'outline'}
              onClick={() => updateTopology('hub', null)}
              disabled={setTopology.isPending || agent.topology_role === 'hub'}
            >
              {agent.topology_role === 'hub' ? 'Hub' : 'Make hub'}
            </Button>
          ) : null}
          {communicationMode === 'hierarchical' ? (
            <select
              value={agent.topology_role === 'leader' ? 'leader' : 'worker'}
              onChange={(event) => onHierarchyRoleChange(event.target.value)}
              disabled={setTopology.isPending}
              className="h-8 rounded-md border border-input bg-background px-2 text-sm"
            >
              <option value="leader">Leader</option>
              <option value="worker">Worker</option>
            </select>
          ) : null}
          {communicationMode === 'ring' ? (
            <Input
              type="number"
              min={1}
              step={1}
              value={agent.speaking_order ?? ''}
              onChange={(event) => onRingOrderChange(event.target.value)}
              disabled={setTopology.isPending}
              className="h-8 w-24"
              aria-label={`Speaking order for ${agent.display_name}`}
            />
          ) : null}
          <Button
            size="sm"
            variant="outline"
            onClick={onToggleSharing}
            disabled={setSharing.isPending}
          >
            {agent.share_group_workspace ? 'Unshare workspace' : 'Share workspace'}
          </Button>
          <Button size="sm" variant="outline" onClick={onMute} disabled={muteAgent.isPending}>
            {isMuted ? 'Unmute' : 'Mute'}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => setConfirmRemoveOpen(true)}
            disabled={removeAgent.isPending}
            className="text-destructive hover:bg-destructive/10 hover:text-destructive"
          >
            Remove
          </Button>
        </div>
      </div>
      {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
      <ConfirmDialog
        open={confirmRemoveOpen}
        onOpenChange={setConfirmRemoveOpen}
        title={`Remove ${agent.display_name}?`}
        description="This agent will no longer participate in the group."
        confirmLabel="Remove"
        destructive
        onConfirm={onRemove}
      />
    </li>
  )
}

interface AddUserRowProps {
  user: UserRead
  groupId: string
}

function AddUserRow({ user, groupId }: AddUserRowProps) {
  const addMember = useAddGroupMember()
  const [error, setError] = useState<string | null>(null)

  const onAdd = () => {
    setError(null)
    addMember.mutate(
      { groupId, userId: user.id },
      { onError: (err) => setError(errorMessage(err, 'Failed to add member')) },
    )
  }

  return (
    <li className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2">
      <div className="min-w-0">
        <p className="truncate text-sm font-medium">{user.name}</p>
        <p className="truncate text-xs text-muted-foreground">{user.email}</p>
        {error ? <p className="text-xs text-destructive">{error}</p> : null}
      </div>
      <Button size="sm" onClick={onAdd} disabled={addMember.isPending}>
        Add
      </Button>
    </li>
  )
}

interface AddAgentRowProps {
  agent: AgentRead
  groupId: string
}

function AddAgentRow({ agent, groupId }: AddAgentRowProps) {
  const addAgent = useAddAgentToGroup()
  const [shareGroupWorkspace, setShareGroupWorkspace] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const onAdd = () => {
    setError(null)
    addAgent.mutate(
      { groupId, agentId: agent.id, shareGroupWorkspace },
      { onError: (err) => setError(errorMessage(err, 'Failed to add agent')) },
    )
  }

  return (
    <li className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2">
      <div className="min-w-0">
        <p className="truncate text-sm font-medium">{agent.name}</p>
        <p className="truncate text-xs text-muted-foreground">{agent.description || 'No description.'}</p>
        <label className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
          <input
            type="checkbox"
            checked={shareGroupWorkspace}
            onChange={(event) => setShareGroupWorkspace(event.target.checked)}
          />
          Allow group workspace
        </label>
        {error ? <p className="text-xs text-destructive">{error}</p> : null}
      </div>
      <Button
        size="sm"
        onClick={onAdd}
        disabled={addAgent.isPending}
        className="self-start"
      >
        Add
      </Button>
    </li>
  )
}

interface GroupMembersTabProps {
  groupId: string
}

export function GroupMembersTab({ groupId }: GroupMembersTabProps) {
  const [userQuery, setUserQuery] = useState('')

  const group = useGroup(groupId)
  const members = useGroupMembers(groupId)
  const groupAgents = useGroupAgents(groupId)
  const userCandidates = useGroupMemberCandidates(groupId, userQuery)
  const agents = useAgents()

  const agentCandidates = useMemo(() => {
    const existing = new Set((groupAgents.data ?? []).map((agent) => agent.agent_id))
    return (agents.data ?? []).filter((agent) => !existing.has(agent.id))
  }, [agents.data, groupAgents.data])

  const isLoading = group.isLoading || members.isLoading || groupAgents.isLoading
  const loadError = group.error ?? members.error ?? groupAgents.error
  const mutedAgentIds = group.data?.muted_agent_ids ?? []

  if (loadError) {
    return (
      <div className="text-sm text-destructive">Failed to load members: {String(loadError)}</div>
    )
  }
  if (isLoading) {
    return <div className="text-sm text-muted-foreground">Loading…</div>
  }

  return (
    <div className="mx-auto grid max-w-6xl gap-6 lg:grid-cols-[1fr_360px]">
      <section className="space-y-6">
        <div className="space-y-3">
          <div>
            <h2 className="text-sm font-semibold">Human members</h2>
            <p className="text-xs text-muted-foreground">
              Mute or remove any human member in this group.
            </p>
          </div>
          <ul className="space-y-3">
            {(members.data ?? []).map((member) => (
              <MemberRow key={member.user_id} member={member} groupId={groupId} />
            ))}
          </ul>
        </div>

        <div className="space-y-3">
          <div>
            <h2 className="text-sm font-semibold">Agent members</h2>
            <p className="text-xs text-muted-foreground">
              Mute or remove any agent that participates in this group. Use
              "Share workspace" to control access to the group workspace.
            </p>
          </div>
          {(groupAgents.data ?? []).length === 0 ? (
            <p className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
              No agents are currently in this group.
            </p>
          ) : (
            <ul className="space-y-3">
              {(groupAgents.data ?? []).map((agent) => (
                <AgentRow
                  key={agent.agent_id}
                  agent={agent}
                  groupId={groupId}
                  isMuted={mutedAgentIds.includes(agent.agent_id)}
                  communicationMode={group.data?.communication_mode ?? 'mesh'}
                />
              ))}
            </ul>
          )}
        </div>
      </section>

      <aside className="space-y-6">
        <section className="rounded-lg border border-border bg-card p-4">
          <div className="space-y-1">
            <h2 className="text-sm font-semibold">Communication topology</h2>
            <p className="text-xs text-muted-foreground">
              Current mode: {group.data?.communication_mode ?? 'mesh'}. Configure each
              agent's role or order in the Agent members list.
            </p>
          </div>
        </section>

        <section className="space-y-3 rounded-lg border border-border bg-card p-4">
          <div>
            <h2 className="text-sm font-semibold">Add human member</h2>
            <p className="text-xs text-muted-foreground">Search by name or email.</p>
          </div>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2 top-2.5 h-4 w-4 text-muted-foreground" />
            <Input
              value={userQuery}
              onChange={(event) => setUserQuery(event.target.value)}
              placeholder="Search users"
              className="pl-8"
            />
          </div>
          <ul className="space-y-2">
            {(userCandidates.data ?? [])
              .filter(
                (user) => !(members.data ?? []).some((member) => member.user_id === user.id),
              )
              .map((user) => (
                <AddUserRow key={user.id} user={user} groupId={groupId} />
              ))}
          </ul>
        </section>

        <section className="space-y-3 rounded-lg border border-border bg-card p-4">
          <div>
            <h2 className="text-sm font-semibold">Add agent</h2>
            <p className="text-xs text-muted-foreground">Add any of your active agents.</p>
          </div>
          {agentCandidates.length === 0 ? (
            <p className="text-xs text-muted-foreground">No available agents to add.</p>
          ) : (
            <ul className="space-y-2">
              {agentCandidates.map((agent) => (
                <AddAgentRow key={agent.id} agent={agent} groupId={groupId} />
              ))}
            </ul>
          )}
        </section>
      </aside>
    </div>
  )
}
