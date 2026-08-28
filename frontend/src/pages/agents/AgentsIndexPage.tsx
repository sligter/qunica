import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Bot, Cpu, Plus, Sparkles, Terminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { AgentAvatar } from '@/components/chat/AgentAvatar'
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

export function AgentsIndexPage() {
  const { t } = useTranslation(['agents', 'common'])
  const agents = useAgents()
  const [query, setQuery] = useState('')

  const list = agents.data ?? []
  const listKey = list.map((a) => a.id).join(',')
  // Prompt text is fair game: agents are remembered by what they do, and the
  // system prompt is the most honest one-line summary of that.
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return list
    return list.filter(
      (a) =>
        a.name.toLowerCase().includes(q) ||
        a.description?.toLowerCase().includes(q) ||
        a.system_prompt?.toLowerCase().includes(q),
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- list is stable per query cache entry; keying on ids avoids re-running the filter on unrelated renders
  }, [listKey, query])
  const acpCount = list.filter((a) => a.runtime_kind === 'acp').length
  const totalMountedSkills = list.reduce((acc, a) => acc + (a.skill_ids?.length ?? 0), 0)

  return (
    <DetailShell
      title={t('agents:list.selectTitle')}
      subtitle={t('agents:list.selectDescription')}
      measure="wide"
      actions={
        <>
          {list.length > 0 ? (
            <SearchInput value={query} onChange={setQuery} label={t('agents:search')} />
          ) : null}
          <Button size="sm" asChild>
            <Link to="/agents/new">
              <Plus className="mr-1.5 h-3.5 w-3.5" />
              {t('agents:new')}
            </Link>
          </Button>
        </>
      }
    >
      {agents.isLoading ? (
        <EntityIndexSkeleton />
      ) : agents.error ? (
        <IndexErrorState
          title={t('agents:loadError')}
          detail={agents.error instanceof Error ? agents.error.message : undefined}
          onRetry={() => void agents.refetch()}
          retryLabel={t('common:actions.retry')}
        />
      ) : list.length === 0 ? (
        <EntityEmptyState
          icon={Bot}
          title={t('agents:empty')}
          description={t('agents:form.createSubtitle')}
          actionLabel={
            <>
              <Plus className="h-4 w-4" />
              {t('agents:new')}
            </>
          }
          actionTo="/agents/new"
        />
      ) : (
        <div className="space-y-6">
          {/* Metric Cards Row */}
          <MetricRow>
            <MetricCard
              label={t('agents:title')}
              value={list.length}
              icon={Bot}
              tone="primary"
            />
            <MetricCard
              label={t('agents:runtime.acpLabel')}
              value={acpCount}
              icon={Terminal}
              tone="info"
            />
            <MetricCard
              label={t('agents:detail.mountedSkills')}
              value={totalMountedSkills}
              icon={Sparkles}
              tone="warning"
            />
          </MetricRow>

          {/* Gallery Grid */}
          <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-2 xl:grid-cols-3">
            {filtered.map((agent) => {
              const mountedCount = agent.skill_ids?.length ?? 0
              return (
                <EntityCard
                  key={agent.id}
                  to={`/agents/${agent.id}`}
                  editTo={`/agents/${agent.id}?edit=1`}
                  title={agent.name}
                  avatarIcon={<AgentAvatar name={agent.name} avatarUrl={agent.avatar_url} />}
                  avatarClass="bg-transparent p-0 shadow-none"
                  description={
                    agent.description || agent.system_prompt || t('common:state.noDescription')
                  }
                  stats={[
                    ...(mountedCount > 0
                      ? [{ key: 'mounted', icon: Sparkles, content: mountedCount }]
                      : []),
                    ...(agent.runtime_kind === 'acp'
                      ? [{ key: 'acp', icon: Cpu, content: 'ACP' }]
                      : []),
                  ]}
                />
              )
            })}
            {filtered.length === 0 ? (
              <NoMatchesState message={t('agents:noMatches', '没有匹配的 Agent。')} />
            ) : null}
          </div>
        </div>
      )}
    </DetailShell>
  )
}
