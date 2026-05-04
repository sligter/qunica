import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { ArrowLeft, Bot, Search, UserRound } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useAddAgentToGroup } from '@/hooks/useAddAgentToGroup'
import { useAgents } from '@/hooks/useAgents'
import { useGroup, useUpdateGroup } from '@/hooks/useGroups'
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
  useSetGroupAgentWorkspaceSharing,
} from '@/hooks/useGroupAgentActions'
import { ApiError } from '@/lib/api'
import type { AgentRead, GroupAgentRead, GroupMemberRead, UserRead } from '@/types/api'

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

  const onMute = () => {
    setError(null)
    muteMember.mutate(
      { groupId, userId: member.user_id, muted: !member.is_muted },
      { onError: (err) => setError(errorMessage(err, 'Failed to update member mute')) },
    )
  }

  const onRemove = () => {
    if (!confirm(`Remove ${member.display_name} from this group?`)) return
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
            onClick={onRemove}
            disabled={removeMember.isPending}
            className="text-red-600 hover:bg-red-50 hover:text-red-700"
          >
            Remove
          </Button>
        </div>
      </div>
      {error ? <p className="mt-2 text-xs text-red-600">{error}</p> : null}
    </li>
  )
}

interface AgentRowProps {
  agent: GroupAgentRead
  groupId: string
  isMuted: boolean
}

function AgentRow({ agent, groupId, isMuted }: AgentRowProps) {
  const muteAgent = useMuteGroupAgent()
  const removeAgent = useRemoveGroupAgent()
  const setSharing = useSetGroupAgentWorkspaceSharing()
  const [error, setError] = useState<string | null>(null)

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

  const onRemove = () => {
    if (!confirm(`Remove ${agent.display_name} from this group?`)) return
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
              {agent.share_group_workspace ? <Badge variant="secondary">Group workspace</Badge> : null}
            </div>
            <p className="text-xs text-muted-foreground">Agent ID: {agent.agent_id}</p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
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
            onClick={onRemove}
            disabled={removeAgent.isPending}
            className="text-red-600 hover:bg-red-50 hover:text-red-700"
          >
            Remove
          </Button>
        </div>
      </div>
      {error ? <p className="mt-2 text-xs text-red-600">{error}</p> : null}
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
        {error ? <p className="text-xs text-red-600">{error}</p> : null}
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
  const [shareGroupWorkspace, setShareGroupWorkspace] = useState(false)
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
        {error ? <p className="text-xs text-red-600">{error}</p> : null}
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

export function GroupMembersPage() {
  const { groupId } = useParams<{ groupId: string }>()
  const [userQuery, setUserQuery] = useState('')

  const group = useGroup(groupId)
  const members = useGroupMembers(groupId)
  const groupAgents = useGroupAgents(groupId)
  const userCandidates = useGroupMemberCandidates(groupId, userQuery)
  const agents = useAgents()
  const updateGroup = useUpdateGroup(groupId ?? '')

  const agentCandidates = useMemo(() => {
    const existing = new Set((groupAgents.data ?? []).map((agent) => agent.agent_id))
    return (agents.data ?? []).filter((agent) => !existing.has(agent.id))
  }, [agents.data, groupAgents.data])

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">No group selected.</div>
  }

  const isLoading = group.isLoading || members.isLoading || groupAgents.isLoading
  const loadError = group.error ?? members.error ?? groupAgents.error
  const mutedAgentIds = group.data?.muted_agent_ids ?? []

  const toggleAllowFreeMention = () => {
    if (!group.data) return
    updateGroup.mutate({ allow_agent_free_mention: !group.data.allow_agent_free_mention })
  }

  return (
    <div className="flex h-full flex-col bg-background">
      <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b border-border px-6">
        <div className="flex min-w-0 items-center gap-3">
          <Button variant="ghost" size="icon" asChild aria-label="Back to group chat">
            <Link to={`/groups/${groupId}`}>
              <ArrowLeft className="h-4 w-4" />
            </Link>
          </Button>
          <div className="min-w-0">
            <h1 className="truncate text-base font-semibold">Manage members</h1>
            <p className="truncate text-xs text-muted-foreground">{group.data?.name}</p>
          </div>
        </div>
      </header>

      {loadError ? (
        <div className="p-6 text-sm text-red-600">Failed to load members: {String(loadError)}</div>
      ) : null}
      {isLoading ? <div className="p-6 text-sm text-muted-foreground">Loading…</div> : null}

      {!isLoading && !loadError ? (
        <main className="flex-1 overflow-y-auto p-6">
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
                    Mute or remove any agent that participates in this group.
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
                      />
                    ))}
                  </ul>
                )}
              </div>
            </section>

            <aside className="space-y-6">
              <section className="rounded-lg border border-border bg-card p-4">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h2 className="text-sm font-semibold">Agent @mentions</h2>
                    <p className="mt-1 text-xs text-muted-foreground">
                      Allow agents to freely @ any group member in replies.
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={toggleAllowFreeMention}
                    disabled={updateGroup.isPending || !group.data}
                  >
                    {group.data?.allow_agent_free_mention ? 'Disable' : 'Enable'}
                  </Button>
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
        </main>
      ) : null}
    </div>
  )
}
