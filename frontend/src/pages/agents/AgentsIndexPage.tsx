import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Bot, Cpu, Plus, Sparkles, Terminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { SearchInput } from '@/components/ui/search-input'
import { useAgents } from '@/hooks/useAgents'
import { formatResourceStatus } from '@/i18n/resourceStatus'

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
  const activeCount = list.filter((a) => a.status === 'active').length
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
      <div className="space-y-6">
        {/* Metric Cards Row */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('agents:title')}</span>
              <Bot className="h-4 w-4 text-primary/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{list.length}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('agents:detail.status')}</span>
              <span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-success">{activeCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('agents:runtime.acpLabel')}</span>
              <Terminal className="h-4 w-4 text-info/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-info">{acpCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('agents:detail.mountedSkills')}</span>
              <Sparkles className="h-4 w-4 text-warning-foreground/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{totalMountedSkills}</p>
          </div>
        </div>

        {/* Gallery Grid or Empty State */}
        {list.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 bg-card/30 p-12 text-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary">
              <Bot className="h-6 w-6" />
            </div>
            <h3 className="text-base font-semibold">{t('agents:empty')}</h3>
            <p className="mt-1 max-w-sm text-sm text-muted-foreground">
              {t('agents:form.createSubtitle')}
            </p>
            <Button className="mt-6 gap-2" asChild>
              <Link to="/agents/new">
                <Plus className="h-4 w-4" />
                {t('agents:new')}
              </Link>
            </Button>
          </div>
        ) : (
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
                  statusLabel={formatResourceStatus(agent.status, t)}
                  statusActive={agent.status === 'active'}
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
              <p className="col-span-full py-12 text-center text-sm text-muted-foreground">
                {t('agents:noMatches', '没有匹配的 Agent。')}
              </p>
            ) : null}
          </div>
        )}
      </div>
    </DetailShell>
  )
}
