import { useMemo, useState } from 'react'
import { ArrowLeft, Search } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { PageState } from '@/components/ui/page-state'
import { Panel } from '@/components/ui/panel'
import { useAddAgentToGroup } from '@/hooks/useAddAgentToGroup'
import { useAgents } from '@/hooks/useAgents'
import { useGroup } from '@/hooks/useGroups'
import { useAddGroupMember, useGroupMemberCandidates, useGroupMembers, useMuteGroupMember, useRemoveGroupMember } from '@/hooks/useGroupMembers'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useMuteGroupAgent, useRemoveGroupAgent, useSetGroupAgentTopology, useSetGroupAgentWorkspaceMode } from '@/hooks/useGroupAgentActions'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'
import { navItemClass } from '@/lib/navItemClass'
import { cn } from '@/lib/utils'
import type { AgentRead, GroupAgentRead, GroupCommunicationMode, GroupMemberRead, GroupTopologyRole, GroupWorkspaceMode, UserRead } from '@/types/api'

type Filter = 'all' | 'human' | 'agent' | 'muted'
type Entry = { kind: 'human'; member: GroupMemberRead } | { kind: 'agent'; agent: GroupAgentRead; muted: boolean }
const NO_TOPOLOGY_ROLE = '__none__'

const communicationModeKeys = {
  mesh: 'members.modes.mesh',
  star: 'members.modes.star',
  hierarchical: 'members.modes.hierarchical',
  ring: 'members.modes.ring',
} as const satisfies Record<GroupCommunicationMode, string>
const communicationModes = Object.keys(communicationModeKeys) as GroupCommunicationMode[]

function isCommunicationMode(value: string): value is GroupCommunicationMode {
  return communicationModes.some((mode) => mode === value)
}

const topologyRoleKeys = {
  hub: 'members.hub',
  leader: 'members.leader',
  worker: 'members.worker',
} as const satisfies Record<GroupTopologyRole, string>
const topologyRoles = Object.keys(topologyRoleKeys) as GroupTopologyRole[]

function isTopologyRole(value: string): value is GroupTopologyRole {
  return topologyRoles.some((role) => role === value)
}

const workspaceModeKeys = {
  group: 'members.workspaceModes.group',
  group_and_self: 'members.workspaceModes.groupAndSelf',
  self: 'members.workspaceModes.self',
} as const satisfies Record<GroupWorkspaceMode, string>
const workspaceModeHintKeys = {
  group: 'members.workspaceModes.groupHint',
  group_and_self: 'members.workspaceModes.groupAndSelfHint',
  self: 'members.workspaceModes.selfHint',
} as const satisfies Record<GroupWorkspaceMode, string>
const workspaceModes = Object.keys(workspaceModeKeys) as GroupWorkspaceMode[]

function isWorkspaceMode(value: string): value is GroupWorkspaceMode {
  return workspaceModes.some((mode) => mode === value)
}

/** Resolve the local paths an agent addresses under its current mode. */
function useAgentWorkspacePaths(groupId: string, agentId: string, mode: GroupWorkspaceMode) {
  const group = useGroup(groupId)
  const agents = useAgents()
  const workspaces = useWorkspaces()
  const localPath = (workspaceId: string | null | undefined) =>
    (workspaces.data ?? []).find((workspace) => workspace.id === workspaceId)?.local_path ?? null
  const own = localPath((agents.data ?? []).find((agent) => agent.id === agentId)?.workspace_id)
  const shared = localPath(group.data?.workspace_id)
  return {
    primary: mode === 'self' ? own : shared,
    mount: mode === 'group_and_self' ? own : null,
  }
}

function entryKey(entry: Entry) { return `${entry.kind}:${entry.kind === 'agent' ? entry.agent.agent_id : entry.member.user_id}` }
function entryName(entry: Entry) { return entry.kind === 'agent' ? entry.agent.display_name : entry.member.display_name }
function entryMuted(entry: Entry) { return entry.kind === 'agent' ? entry.muted : entry.member.is_muted }

function AddUser({ user, groupId }: { user: UserRead; groupId: string }) {
  const { t } = useTranslation('groups')
  const add = useAddGroupMember()
  return <li className="flex items-center justify-between gap-3 border-b border-border py-2 last:border-0"><div className="flex min-w-0 items-center gap-2"><AgentAvatar name={user.name} kind="user" avatarUrl={user.avatar_url} /><div className="min-w-0"><p className="truncate text-sm font-medium">{user.name}</p><p className="truncate text-xs text-muted-foreground">{user.email}</p></div></div><Button size="sm" onClick={() => add.mutate({ groupId, userId: user.id })} disabled={add.isPending}>{add.isPending ? t('members.adding') : t('members.add')}</Button></li>
}

function AddAgent({ agent, groupId, defaultWorkspaceMode }: { agent: AgentRead; groupId: string; defaultWorkspaceMode: GroupWorkspaceMode }) {
  const { t } = useTranslation('groups')
  const add = useAddAgentToGroup()
  const [workspaceMode, setWorkspaceMode] = useState<GroupWorkspaceMode>(defaultWorkspaceMode)
  return <li className="flex items-start justify-between gap-3 border-b border-border py-2 last:border-0"><div className="min-w-0"><p className="truncate text-sm font-medium">{agent.name}</p><p className="truncate text-xs text-muted-foreground">{agent.description || t('members.noDescription')}</p><label className="mt-1.5 block space-y-1 text-xs text-muted-foreground"><span>{t('members.allowWorkspace')}</span><select aria-label={t('members.workspaceAccess')} className="h-8 w-full rounded-md border border-input bg-background px-2 text-xs" value={workspaceMode} onChange={(event) => { if (isWorkspaceMode(event.target.value)) setWorkspaceMode(event.target.value) }}>{workspaceModes.map((value) => <option key={value} value={value}>{t(workspaceModeKeys[value])}</option>)}</select></label></div><Button size="sm" onClick={() => add.mutate({ groupId, agentId: agent.id, workspaceMode })} disabled={add.isPending}>{add.isPending ? t('members.adding') : t('members.add')}</Button></li>
}

/**
 * Pick which workspace roots this agent addresses, and show where they land.
 * The resolved paths matter: an isolated agent silently stops seeing group
 * files, so the consequence of the choice has to be visible next to it.
 */
function AgentWorkspaceAccess({ groupId, agent, onError }: { groupId: string; agent: GroupAgentRead; onError: (error: unknown) => void }) {
  const { t } = useTranslation('groups')
  const setWorkspaceMode = useSetGroupAgentWorkspaceMode()
  const mode = agent.workspace_mode
  const { primary, mount } = useAgentWorkspacePaths(groupId, agent.agent_id, mode)
  const notConfigured = t('members.workspaceNotConfigured')
  return (
    <div className="space-y-2">
      <div>
        <p className="text-sm font-medium">{t('members.workspaceAccess')}</p>
        <p className="text-xs text-muted-foreground">{t('members.workspaceAccessDescription')}</p>
      </div>
      <select
        aria-label={t('members.workspaceAccess')}
        className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
        value={mode}
        disabled={setWorkspaceMode.isPending}
        onChange={(event) => {
          const next = event.target.value
          if (!isWorkspaceMode(next) || next === mode) return
          setWorkspaceMode.mutate({ groupId, agentId: agent.agent_id, workspaceMode: next }, { onError })
        }}
      >
        {workspaceModes.map((value) => <option key={value} value={value}>{t(workspaceModeKeys[value])}</option>)}
      </select>
      <p className="text-xs text-muted-foreground">{t(workspaceModeHintKeys[mode])}</p>
      <p className="truncate text-2xs text-muted-foreground" title={primary ?? undefined}>{t('members.workspacePrimary', { location: primary ?? notConfigured })}</p>
      {mount ? <p className="truncate text-2xs text-muted-foreground" title={mount}>{t('members.workspaceMount', { location: mount })}</p> : null}
    </div>
  )
}

function EntryRow({ entry, active, mode, onSelect }: { entry: Entry; active: boolean; mode: GroupCommunicationMode; onSelect: () => void }) {
  const { t, i18n } = useTranslation('groups')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const agent = entry.kind === 'agent'
  const topologyRole = agent ? (entry.agent.topology_role as string | null) : null
  const hierarchicalTag = mode === 'hierarchical' && topologyRole
    ? isTopologyRole(topologyRole)
      ? t(topologyRoleKeys[topologyRole])
      : t('members.unknownTopologyRole', { value: topologyRole })
    : false
  // Only non-default workspace modes get a tag; badging the default is noise.
  const workspaceTag = agent && entry.agent.workspace_mode !== 'group' && t(workspaceModeKeys[entry.agent.workspace_mode])
  const tags = (agent
    ? [entry.muted && t('members.muted'), workspaceTag, mode === 'star' && entry.agent.topology_role === 'hub' && t('members.hub'), hierarchicalTag, mode === 'ring' && entry.agent.speaking_order !== null && `#${formatNumber(entry.agent.speaking_order, language)}`]
    : [entry.member.is_muted && t('members.muted')]
  ).filter((tag): tag is string => Boolean(tag))
  const rawRole = agent ? entry.agent.role : entry.member.role
  const knownRole = rawRole === 'owner' || rawRole === 'admin' || rawRole === 'worker' || rawRole === 'hub' || rawRole === 'participant' || rawRole === 'member' || rawRole === 'agent'
  const role = knownRole ? t(`members.${rawRole}`) : rawRole
  return <li><button type="button" onClick={onSelect} aria-current={active || undefined} className={navItemClass(active, 'items-center gap-3 px-3 py-2.5')}>{agent ? <AgentAvatar name={entry.agent.display_name} avatarUrl={entry.agent.avatar_url} /> : <AgentAvatar name={entry.member.display_name} kind="user" avatarUrl={entry.member.avatar_url} />}<div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className={cn('truncate text-sm', active ? 'font-semibold' : 'font-medium')}>{entryName(entry)}</span><Badge variant="outline" className="shrink-0">{role || t('members.agent')}</Badge></div>{tags.length > 0 ? <div className="mt-1 flex flex-wrap gap-1">{tags.map((tag) => <Badge key={tag} variant="secondary">{tag}</Badge>)}</div> : null}</div></button></li>
}

function Details({ entry, groupId, mode, onRemoved }: { entry: Entry; groupId: string; mode: GroupCommunicationMode; onRemoved: () => void }) {
  const { t } = useTranslation('groups')
  const muteHuman = useMuteGroupMember()
  const removeHuman = useRemoveGroupMember()
  const muteAgent = useMuteGroupAgent()
  const removeAgent = useRemoveGroupAgent()
  const topology = useSetGroupAgentTopology()
  const [error, setError] = useState<{ key: string; detail?: string } | null>(null)
  const [confirm, setConfirm] = useState(false)
  const agent = entry.kind === 'agent'
  const topologyRole = agent ? (entry.agent.topology_role as string | null) : null
  const muted = entryMuted(entry)
  const fail = (key: string) => (nextError: unknown) =>
    setError({ key, detail: nextError instanceof ApiError ? nextError.message : undefined })
  const updateTopology = (role?: GroupTopologyRole | null, order?: number | null) => {
    if (agent) {
      topology.mutate(
        { groupId, agentId: entry.agent.agent_id, topologyRole: role, speakingOrder: order },
        { onError: fail('members.errors.topology') },
      )
    }
  }
  const mute = () => {
    setError(null)
    if (agent) {
      muteAgent.mutate(
        { groupId, agentId: entry.agent.agent_id, muted: !muted },
        { onError: fail('members.errors.agentMute') },
      )
    } else {
      muteHuman.mutate(
        { groupId, userId: entry.member.user_id, muted: !muted },
        { onError: fail('members.errors.memberMute') },
      )
    }
  }
  const remove = () => {
    setError(null)
    if (agent) {
      removeAgent.mutate(
        { groupId, agentId: entry.agent.agent_id },
        { onSuccess: onRemoved, onError: fail('members.errors.removeAgent') },
      )
    } else {
      removeHuman.mutate(
        { groupId, userId: entry.member.user_id },
        { onSuccess: onRemoved, onError: fail('members.errors.removeMember') },
      )
    }
  }
  const errorText = error
    ? error.detail
      ? t('members.errors.detail', { message: t(error.key), detail: error.detail })
      : t(error.key)
    : null

  return (
    <Card asChild className="space-y-4 p-4">
      <section>
      <div className="flex items-start gap-3">
        {agent ? <AgentAvatar name={entry.agent.display_name} avatarUrl={entry.agent.avatar_url} size="lg" /> : <AgentAvatar name={entry.member.display_name} kind="user" avatarUrl={entry.member.avatar_url} size="lg" />}
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold">{entryName(entry)}</h2>
          <p className="mt-0.5 truncate text-xs text-muted-foreground">
            {agent
              ? t('members.agentId', { id: entry.agent.agent_id })
              : t('members.userId', { id: entry.member.user_id })}
          </p>
        </div>
      </div>
      {agent ? (
        <div className="space-y-4 border-t border-border pt-4">
          <AgentWorkspaceAccess groupId={groupId} agent={entry.agent} onError={fail('members.errors.workspace')} />
          {mode === 'star' ? <div className="flex items-center justify-between gap-3"><div><p className="text-sm font-medium">{t('members.starTopology')}</p><p className="text-xs text-muted-foreground">{t('members.setAsHub')}</p></div><Button size="sm" variant={entry.agent.topology_role === 'hub' ? 'default' : 'outline'} disabled={topology.isPending || entry.agent.topology_role === 'hub'} onClick={() => updateTopology('hub', null)}>{entry.agent.topology_role === 'hub' ? t(topologyRoleKeys.hub) : t('members.makeHub')}</Button></div> : null}
          {mode === 'hierarchical' ? <label className="block space-y-1.5 text-sm font-medium">{t('members.hierarchyRole')}<select className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={topologyRole ?? NO_TOPOLOGY_ROLE} disabled={topology.isPending} onChange={(event) => { const role = event.target.value; if (role === 'leader' || role === 'worker') updateTopology(role, null) }}><option value={NO_TOPOLOGY_ROLE} disabled>{t('members.noTopologyRole')}</option>{topologyRole && topologyRole !== 'leader' && topologyRole !== 'worker' ? <option value={topologyRole}>{isTopologyRole(topologyRole) ? t(topologyRoleKeys[topologyRole]) : t('members.unknownTopologyRole', { value: topologyRole })}</option> : null}<option value="leader">{t(topologyRoleKeys.leader)}</option><option value="worker">{t(topologyRoleKeys.worker)}</option></select></label> : null}
          {mode === 'ring' ? <label className="block space-y-1.5 text-sm font-medium">{t('members.speakingOrder')}<Input type="number" min={1} value={entry.agent.speaking_order ?? ''} disabled={topology.isPending} onChange={(event) => updateTopology(null, event.target.value === '' ? null : Number(event.target.value))} /></label> : null}
        </div>
      ) : null}
      <div className="flex justify-between gap-2 border-t border-border pt-4">
        <Button size="sm" variant="outline" onClick={mute} disabled={muteHuman.isPending || muteAgent.isPending}>{muted ? t('members.unmute') : t('members.mute')}</Button>
        <Button size="sm" variant="outline" className="text-destructive hover:bg-destructive/10 hover:text-destructive" onClick={() => setConfirm(true)} disabled={removeHuman.isPending || removeAgent.isPending}>{t('members.remove')}</Button>
      </div>
      {errorText ? <p className="text-xs text-destructive" role="alert">{errorText}</p> : null}
      <ConfirmDialog open={confirm} onOpenChange={setConfirm} title={t('members.removeTitle', { name: entryName(entry) })} description={t('members.removeDescription')} confirmLabel={t('members.remove')} destructive onConfirm={remove} />
      </section>
    </Card>
  )
}

export function GroupMembersTab({
  groupId,
  compact = false,
}: {
  groupId: string
  compact?: boolean
}) {
  const { t, i18n } = useTranslation('groups')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const [query, setQuery] = useState('')
  const [userQuery, setUserQuery] = useState('')
  const [agentQuery, setAgentQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [selected, setSelected] = useState<string | null>(null)
  // Below xl the two-column grid stacks, which drops a selected member's
  // details far below the fold; there the panes swap instead — selecting a
  // member replaces the list, and the explicit back control restores it.
  // Keyed off selection state rather than matchMedia: jsdom's is always false,
  // and `max-xl:` variants keep wide windows side-by-side either way.
  const [panesSwapped, setPanesSwapped] = useState(false)
  const exclusive = selected !== null && (compact || panesSwapped)
  const group = useGroup(groupId)
  const humans = useGroupMembers(groupId)
  const groupAgents = useGroupAgents(groupId)
  const userCandidates = useGroupMemberCandidates(groupId, userQuery)
  const agents = useAgents()
  const mutedAgentIds = useMemo(
    () => group.data?.muted_agent_ids ?? [],
    [group.data?.muted_agent_ids],
  )
  const mode = group.data?.communication_mode ?? 'mesh'
  const defaultWorkspaceMode: GroupWorkspaceMode =
    group.data?.auto_share_workspace_with_new_agents === false ? 'self' : 'group'
  const entries = useMemo<Entry[]>(
    () => [
      ...(humans.data ?? []).map((member) => ({ kind: 'human' as const, member })),
      ...(groupAgents.data ?? []).map((agent) => ({
        kind: 'agent' as const,
        agent,
        muted: mutedAgentIds.includes(agent.agent_id),
      })),
    ],
    [humans.data, groupAgents.data, mutedAgentIds],
  )
  const visible = useMemo(
    () =>
      entries.filter((entry) => {
        const name = entryName(entry).toLowerCase()
        return (
          (!query || name.includes(query.trim().toLowerCase())) &&
          (filter === 'all' ||
            filter === entry.kind ||
            (filter === 'muted' && entryMuted(entry)))
        )
      }),
    [entries, filter, query],
  )
  const current = entries.find((entry) => entryKey(entry) === selected) ?? null
  const availableAgents = useMemo(() => {
    const existing = new Set((groupAgents.data ?? []).map((agent) => agent.agent_id))
    return (agents.data ?? []).filter((agent) => !existing.has(agent.id))
  }, [agents.data, groupAgents.data])
  const visibleAvailableAgents = useMemo(() => {
    const normalized = agentQuery.trim().toLowerCase()
    if (!normalized) return availableAgents
    return availableAgents.filter((agent) =>
      `${agent.name}\n${agent.description ?? ''}`.toLowerCase().includes(normalized),
    )
  }, [agentQuery, availableAgents])
  const filters: Array<[Filter, string]> = [
    ['all', t('members.all')],
    ['human', t('members.human')],
    ['agent', t('members.agents')],
    ['muted', t('members.muted')],
  ]

  if (group.error || humans.error || groupAgents.error) {
    return <PageState inset variant="error" className="px-0" title={t('members.loadError')} />
  }
  if (group.isLoading || humans.isLoading || groupAgents.isLoading) {
    return <PageState inset variant="loading" className="px-0" title={t('members.loading')} />
  }

  return (
    <div
      className={cn(
        'grid min-h-[34rem] w-full grid-cols-1 gap-5',
        !compact && 'xl:grid-cols-[minmax(0,1fr)_22rem]',
      )}
    >
      {/* Swapped out only below xl, where the details would otherwise stack
          a screen away; wide windows keep the master/detail grid. */}
      <Card
        asChild
        className={cn(
          'flex min-h-0 flex-col',
          exclusive && (compact ? 'hidden' : 'max-xl:hidden'),
        )}
      >
        <section>
        <div className="space-y-3 border-b border-border p-4">
          <div>
            <h2 className="text-sm font-semibold">{t('members.title')}</h2>
            <p className="text-xs text-muted-foreground">
              {t('members.count', {
                count: entries.length,
                formattedCount: formatNumber(entries.length, language),
              })}
            </p>
          </div>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input className="h-9 pl-8" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t('members.search')} />
          </div>
          <div className="flex flex-wrap gap-1">
            {filters.map(([value, label]) => <Button key={value} size="sm" variant={filter === value ? 'secondary' : 'ghost'} className="h-7" onClick={() => setFilter(value)}>{label}</Button>)}
          </div>
        </div>
        <ul className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-1.5 py-1">
          {visible.map((entry) => <EntryRow key={entryKey(entry)} entry={entry} active={entryKey(entry) === selected} mode={mode} onSelect={() => { setSelected(entryKey(entry)); setPanesSwapped(true) }} />)}
          {visible.length === 0 ? <li className="p-6 text-center text-sm text-muted-foreground">{t('members.noMatches')}</li> : null}
        </ul>
        </section>
      </Card>
      <aside className="space-y-4">
        {exclusive ? (
          <Button
            variant="ghost"
            size="sm"
            className={cn('-ml-2 h-7 gap-1.5 text-xs', !compact && 'xl:hidden')}
            onClick={() => {
              setSelected(null)
              setPanesSwapped(false)
            }}
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            {t('members.backToList')}
          </Button>
        ) : null}
        {current ? (
          <Details entry={current} groupId={groupId} mode={mode} onRemoved={() => setSelected(null)} />
        ) : (
          <>
            <Panel title={t('members.details')} description={t('members.detailsHint')} />
            <Panel title={t('members.topology')} description={t('members.currentMode', { mode: isCommunicationMode(mode as string) ? t(communicationModeKeys[mode]) : (mode as string) })} />
            <Panel title={t('members.addHuman')}>
              <div className="relative"><Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" /><Input className="h-9 pl-8" value={userQuery} onChange={(event) => setUserQuery(event.target.value)} placeholder={t('members.searchUsers')} /></div>
              <ul className="mt-2 max-h-48 overflow-y-auto">{(userCandidates.data ?? []).filter((user) => !(humans.data ?? []).some((member) => member.user_id === user.id)).map((user) => <AddUser key={user.id} user={user} groupId={groupId} />)}</ul>
            </Panel>
            <Panel title={t('members.addAgent')}>
              <div className="relative">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  aria-label={t('manage.agentSearch.placeholder')}
                  className="h-9 pl-8"
                  value={agentQuery}
                  onChange={(event) => setAgentQuery(event.target.value)}
                  placeholder={t('manage.agentSearch.placeholder')}
                />
              </div>
              <ul className="mt-2 max-h-56 overflow-y-auto">
                {visibleAvailableAgents.map((agent) => <AddAgent key={`${agent.id}:${defaultWorkspaceMode}`} agent={agent} groupId={groupId} defaultWorkspaceMode={defaultWorkspaceMode} />)}
                {visibleAvailableAgents.length === 0 ? (
                  <li className="py-2 text-xs text-muted-foreground">
                    {availableAgents.length === 0 ? t('members.noAgents') : t('manage.agentSearch.noMatches')}
                  </li>
                ) : null}
              </ul>
            </Panel>
          </>
        )}
      </aside>
    </div>
  )
}
