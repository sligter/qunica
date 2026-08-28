import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Bot, Folder, HardDrive, Plus, Users } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { DetailShell } from '@/components/layout/DetailShell'
import {
  EntityEmptyState,
  EntityIndexSkeleton,
  IndexErrorState,
  MetricCard,
  MetricRow,
  NoMatchesState,
} from '@/components/layout/EntityIndexParts'
import { Button } from '@/components/ui/button'
import { SearchInput } from '@/components/ui/search-input'
import { useAgents } from '@/hooks/useAgents'
import { useGroups } from '@/hooks/useGroups'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { avatarColorClass } from '@/lib/avatarColor'

export function WorkspacesIndexPage() {
  const { t } = useTranslation(['workspaces', 'common'])
  const workspaces = useWorkspaces()
  const groups = useGroups()
  const agents = useAgents()

  const list = workspaces.data ?? []
  const groupList = groups.data ?? []
  const agentList = agents.data ?? []
  const [query, setQuery] = useState('')
  // Paths are the main thing people search on here — a workspace is known by
  // where it points, not only by its display name.
  const listKey = list.map((w) => w.id).join(',')
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return list
    return list.filter((w) => {
      const target = w.backend_type === 'local' ? w.local_path : w.sandbox_ref
      return w.name.toLowerCase().includes(q) || target?.toLowerCase().includes(q)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- list is stable per query cache entry; keying on ids avoids re-running the filter on unrelated renders
  }, [listKey, query])
  const localCount = list.filter((w) => w.backend_type === 'local').length
  const boundWorkspaces = new Set([
    ...groupList.map((g) => g.workspace_id).filter(Boolean),
    ...agentList.map((a) => a.workspace_id).filter(Boolean),
  ]).size

  return (
    <DetailShell
      title={t('workspaces:list.selectTitle')}
      subtitle={t('workspaces:list.selectDescription')}
      measure="wide"
      actions={
        <>
          {list.length > 0 ? (
            <SearchInput value={query} onChange={setQuery} label={t('workspaces:search')} />
          ) : null}
          <Button size="sm" asChild>
            <Link to="/workspaces/new">
              <Plus className="mr-1.5 h-3.5 w-3.5" />
              {t('workspaces:new')}
            </Link>
          </Button>
        </>
      }
    >
      {workspaces.isLoading ? (
        <EntityIndexSkeleton />
      ) : workspaces.error ? (
        <IndexErrorState
          title={t('workspaces:loadError')}
          detail={workspaces.error instanceof Error ? workspaces.error.message : undefined}
          onRetry={() => void workspaces.refetch()}
          retryLabel={t('common:actions.retry')}
        />
      ) : list.length === 0 ? (
        <EntityEmptyState
          icon={Folder}
          title={t('workspaces:empty')}
          description={t('workspaces:form.createSubtitle')}
          actionLabel={
            <>
              <Plus className="h-4 w-4" />
              {t('workspaces:new')}
            </>
          }
          actionTo="/workspaces/new"
        />
      ) : (
        <div className="space-y-6">
        {/* Metric Cards Row */}
        <MetricRow>
          <MetricCard
            label={t('workspaces:title')}
            value={list.length}
            icon={Folder}
            tone="primary"
          />
          <MetricCard
            label={t('workspaces:backendTypes.local')}
            value={localCount}
            icon={HardDrive}
            tone="info"
          />
          <MetricCard
            label={t('workspaces:detail.usedBy')}
            value={boundWorkspaces}
            icon={Users}
            tone="success"
          />
          <MetricCard
            label={t('workspaces:detail.agent')}
            value={agentList.length}
            icon={Bot}
            tone="warning"
          />
        </MetricRow>

        {/* Gallery Grid */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
            {filtered.map((workspace) => {
              const boundGroups = groupList.filter((g) => g.workspace_id === workspace.id)
              const boundAgents = agentList.filter((a) => a.workspace_id === workspace.id)
              const pathText =
                workspace.backend_type === 'local'
                  ? workspace.local_path || t('workspaces:noLocalPath')
                  : workspace.sandbox_ref || t('workspaces:noSandboxReference')
              const stats = [
                ...(boundGroups.length > 0
                  ? [{ key: 'groups', icon: Users, content: boundGroups.length }]
                  : []),
                ...(boundAgents.length > 0
                  ? [{ key: 'agents', icon: Bot, content: boundAgents.length }]
                  : []),
                ...(boundGroups.length === 0 && boundAgents.length === 0
                  ? [{ key: 'unused', content: t('workspaces:detail.unused') }]
                  : []),
              ]

              return (
                <EntityCard
                  key={workspace.id}
                  to={`/workspaces/${workspace.id}`}
                  // The workspace detail page edits inline — there is no
                  // separate edit form to deep-link into.
                  title={workspace.name}
                  avatarInitial={workspace.name.slice(0, 1).toUpperCase()}
                  avatarClass={avatarColorClass(workspace.id)}
                  statusLabel={formatResourceStatus(workspace.status, t)}
                  statusActive={workspace.status === 'active'}
                  metaBadge={{
                    label: workspace.backend_type,
                    className:
                      'border border-border/60 bg-muted/60 font-mono text-muted-foreground',
                  }}
                  description={<code className="truncate">{pathText}</code>}
                  stats={stats}
                />
              )
            })}
            {filtered.length === 0 ? (
              <NoMatchesState message={t('workspaces:noMatches', '没有匹配的工作区。')} />
            ) : null}
          </div>
        </div>
      )}
    </DetailShell>
  )
}
