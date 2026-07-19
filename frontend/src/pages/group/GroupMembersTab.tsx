import { useMemo, useState } from 'react'
import { Bot, Search, UserRound } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { useAddAgentToGroup } from '@/hooks/useAddAgentToGroup'
import { useAgents } from '@/hooks/useAgents'
import { useGroup } from '@/hooks/useGroups'
import { useAddGroupMember, useGroupMemberCandidates, useGroupMembers, useMuteGroupMember, useRemoveGroupMember } from '@/hooks/useGroupMembers'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useMuteGroupAgent, useRemoveGroupAgent, useSetGroupAgentTopology, useSetGroupAgentWorkspaceSharing } from '@/hooks/useGroupAgentActions'
import { ApiError } from '@/lib/api-v2/client'
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { AgentRead, GroupAgentRead, GroupCommunicationMode, GroupMemberRead, GroupTopologyRole, UserRead } from '@/types/api'

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

function entryKey(entry: Entry) { return `${entry.kind}:${entry.kind === 'agent' ? entry.agent.agent_id : entry.member.user_id}` }
function entryName(entry: Entry) { return entry.kind === 'agent' ? entry.agent.display_name : entry.member.display_name }
function entryMuted(entry: Entry) { return entry.kind === 'agent' ? entry.muted : entry.member.is_muted }

function AddUser({ user, groupId }: { user: UserRead; groupId: string }) {
  const { t } = useTranslation('groups')
  const add = useAddGroupMember()
  return <li className="flex items-center justify-between gap-3 border-b border-border py-2 last:border-0"><div className="min-w-0"><p className="truncate text-sm font-medium">{user.name}</p><p className="truncate text-xs text-muted-foreground">{user.email}</p></div><Button size="sm" onClick={() => add.mutate({ groupId, userId: user.id })} disabled={add.isPending}>{add.isPending ? t('members.adding') : t('members.add')}</Button></li>
}

function AddAgent({ agent, groupId }: { agent: AgentRead; groupId: string }) {
  const { t } = useTranslation('groups')
  const add = useAddAgentToGroup()
  const [share, setShare] = useState(true)
  return <li className="flex items-start justify-between gap-3 border-b border-border py-2 last:border-0"><div className="min-w-0"><p className="truncate text-sm font-medium">{agent.name}</p><p className="truncate text-xs text-muted-foreground">{agent.description || t('members.noDescription')}</p><label className="mt-1.5 flex items-center gap-2 text-xs text-muted-foreground"><input type="checkbox" checked={share} onChange={(event) => setShare(event.target.checked)} />{t('members.allowWorkspace')}</label></div><Button size="sm" onClick={() => add.mutate({ groupId, agentId: agent.id, shareGroupWorkspace: share })} disabled={add.isPending}>{add.isPending ? t('members.adding') : t('members.add')}</Button></li>
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
  const tags = (agent
    ? [entry.muted && t('members.muted'), entry.agent.share_group_workspace && t('members.workspace'), mode === 'star' && entry.agent.topology_role === 'hub' && t('members.hub'), hierarchicalTag, mode === 'ring' && entry.agent.speaking_order !== null && `#${formatNumber(entry.agent.speaking_order, language)}`]
    : [entry.member.is_muted && t('members.muted')]
  ).filter((tag): tag is string => Boolean(tag))
  const rawRole = agent ? entry.agent.role : entry.member.role
  const knownRole = rawRole === 'owner' || rawRole === 'admin' || rawRole === 'worker' || rawRole === 'hub' || rawRole === 'participant' || rawRole === 'member' || rawRole === 'agent'
  const role = knownRole ? t(`members.${rawRole}`) : rawRole
  return <li><button type="button" onClick={onSelect} className={cn('flex w-full items-center gap-3 border-b border-border px-3 py-2.5 text-left transition-colors last:border-0', active ? 'bg-primary/10' : 'hover:bg-card-hover')}><div className={cn('flex h-8 w-8 shrink-0 items-center justify-center rounded-full', agent ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground')}>{agent ? <Bot className="h-4 w-4" /> : <UserRound className="h-4 w-4" />}</div><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className="truncate text-sm font-medium">{entryName(entry)}</span><Badge variant="outline" className="shrink-0">{role || t('members.agent')}</Badge></div>{tags.length > 0 ? <div className="mt-1 flex flex-wrap gap-1">{tags.map((tag) => <Badge key={tag} variant="secondary">{tag}</Badge>)}</div> : null}</div></button></li>
}

function Details({ entry, groupId, mode, onRemoved }: { entry: Entry; groupId: string; mode: GroupCommunicationMode; onRemoved: () => void }) {
  const { t } = useTranslation('groups')
  const muteHuman = useMuteGroupMember()
  const removeHuman = useRemoveGroupMember()
  const muteAgent = useMuteGroupAgent()
  const removeAgent = useRemoveGroupAgent()
  const sharing = useSetGroupAgentWorkspaceSharing()
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
    <section className="space-y-4 rounded-lg border border-border bg-card p-4">
      <div className="flex items-start gap-3">
        <div className={cn('flex h-10 w-10 shrink-0 items-center justify-center rounded-full', agent ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground')}>
          {agent ? <Bot className="h-5 w-5" /> : <UserRound className="h-5 w-5" />}
        </div>
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
          <div className="flex items-center justify-between gap-3">
            <div><p className="text-sm font-medium">{t('members.groupWorkspace')}</p><p className="text-xs text-muted-foreground">{t('members.sharedWorkspaceAccess')}</p></div>
            <Button size="sm" variant="outline" disabled={sharing.isPending} onClick={() => sharing.mutate({ groupId, agentId: entry.agent.agent_id, shareGroupWorkspace: !entry.agent.share_group_workspace }, { onError: fail('members.errors.workspace') })}>{entry.agent.share_group_workspace ? t('members.unshare') : t('members.share')}</Button>
          </div>
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
  )
}

export function GroupMembersTab({ groupId }: { groupId: string }) {
  const { t, i18n } = useTranslation('groups')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const [query, setQuery] = useState('')
  const [userQuery, setUserQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [selected, setSelected] = useState<string | null>(null)
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
  const filters: Array<[Filter, string]> = [
    ['all', t('members.all')],
    ['human', t('members.human')],
    ['agent', t('members.agents')],
    ['muted', t('members.muted')],
  ]

  if (group.error || humans.error || groupAgents.error) {
    return <div className="text-sm text-destructive">{t('members.loadError')}</div>
  }
  if (group.isLoading || humans.isLoading || groupAgents.isLoading) {
    return <div className="text-sm text-muted-foreground">{t('members.loading')}</div>
  }

  return (
    <div className="grid min-h-[34rem] w-full grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_22rem]">
      <section className="flex min-h-0 flex-col rounded-lg border border-border bg-card">
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
        <ul className="min-h-0 flex-1 overflow-y-auto">
          {visible.map((entry) => <EntryRow key={entryKey(entry)} entry={entry} active={entryKey(entry) === selected} mode={mode} onSelect={() => setSelected(entryKey(entry))} />)}
          {visible.length === 0 ? <li className="p-6 text-center text-sm text-muted-foreground">{t('members.noMatches')}</li> : null}
        </ul>
      </section>
      <aside className="space-y-4">
        {current ? (
          <Details entry={current} groupId={groupId} mode={mode} onRemoved={() => setSelected(null)} />
        ) : (
          <>
            <section className="rounded-lg border border-border bg-card p-4"><h2 className="text-sm font-semibold">{t('members.details')}</h2><p className="mt-1 text-xs text-muted-foreground">{t('members.detailsHint')}</p></section>
            <section className="rounded-lg border border-border bg-card p-4"><h2 className="text-sm font-semibold">{t('members.topology')}</h2><p className="mt-1 text-xs text-muted-foreground">{t('members.currentMode', { mode: isCommunicationMode(mode as string) ? t(communicationModeKeys[mode]) : (mode as string) })}</p></section>
            <section className="rounded-lg border border-border bg-card p-4"><h2 className="text-sm font-semibold">{t('members.addHuman')}</h2><div className="relative mt-3"><Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" /><Input className="h-9 pl-8" value={userQuery} onChange={(event) => setUserQuery(event.target.value)} placeholder={t('members.searchUsers')} /></div><ul className="mt-2 max-h-48 overflow-y-auto">{(userCandidates.data ?? []).filter((user) => !(humans.data ?? []).some((member) => member.user_id === user.id)).map((user) => <AddUser key={user.id} user={user} groupId={groupId} />)}</ul></section>
            <section className="rounded-lg border border-border bg-card p-4"><h2 className="text-sm font-semibold">{t('members.addAgent')}</h2><ul className="mt-2 max-h-56 overflow-y-auto">{availableAgents.map((agent) => <AddAgent key={agent.id} agent={agent} groupId={groupId} />)}{availableAgents.length === 0 ? <li className="py-2 text-xs text-muted-foreground">{t('members.noAgents')}</li> : null}</ul></section>
          </>
        )}
      </aside>
    </div>
  )
}
