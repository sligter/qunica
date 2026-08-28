import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { FileCode2, Plus, Sparkles } from 'lucide-react'
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
import { useSkills } from '@/hooks/useSkills'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { avatarColorClass } from '@/lib/avatarColor'

export function SkillsIndexPage() {
  const { t } = useTranslation(['skills', 'common'])
  const skills = useSkills()
  const [query, setQuery] = useState('')

  const list = skills.data ?? []
  const listKey = list.map((s) => s.id).join(',')
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return list
    return list.filter(
      (s) => s.name.toLowerCase().includes(q),
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- list is stable per query cache entry; keying on ids avoids re-running the filter on unrelated renders
  }, [listKey, query])
  const activeCount = list.filter((s) => s.status === 'active').length
  const totalFiles = list.reduce((acc, s) => acc + (s.files?.length ?? 0), 0)

  return (
    <DetailShell
      title={t('skills:list.selectTitle')}
      subtitle={t('skills:list.selectDescription')}
      measure="wide"
      actions={
        <>
          {list.length > 0 ? (
            <SearchInput
              value={query}
              onChange={setQuery}
              label={t('skills:search')}
            />
          ) : null}
          <Button size="sm" asChild>
            <Link to="/skills/new">
              <Plus className="mr-1.5 h-3.5 w-3.5" />
              {t('skills:import')}
            </Link>
          </Button>
        </>
      }
    >
      {skills.isLoading ? (
        <EntityIndexSkeleton />
      ) : skills.error ? (
        <IndexErrorState
          title={t('skills:loadError')}
          detail={skills.error instanceof Error ? skills.error.message : undefined}
          onRetry={() => void skills.refetch()}
          retryLabel={t('common:actions.retry')}
        />
      ) : list.length === 0 ? (
        <EntityEmptyState
          icon={Sparkles}
          title={t('skills:empty')}
          description={t('skills:form.createSubtitle')}
          actionLabel={
            <>
              <Plus className="h-4 w-4" />
              {t('skills:import')}
            </>
          }
          actionTo="/skills/new"
        />
      ) : (
        <div className="space-y-6">
        {/* Metric Cards Row */}
        <MetricRow>
          <MetricCard label={t('skills:title')} value={list.length} icon={Sparkles} tone="primary" />
          <MetricCard
            label={t('common:state.active')}
            value={activeCount}
            tone="success"
            marker={<span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />}
          />
          <MetricCard
            label={t('skills:resources.title')}
            value={totalFiles}
            icon={FileCode2}
            tone="info"
          />
          <MetricCard
            label={t('skills:detail.sourceShort')}
            value={list.filter((s) => s.source !== 'manual').length}
            marker={<span className="font-mono text-2xs uppercase text-muted-foreground">pkg</span>}
          />
        </MetricRow>

        {/* Gallery Grid */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
            {filtered.map((skill) => {
              const fileCount = skill.files?.length ?? 0
              return (
                <EntityCard
                  key={skill.id}
                  to={`/skills/${skill.id}`}
                  editTo={`/skills/${skill.id}?edit=1`}
                  title={skill.name}
                  avatarInitial={skill.name.slice(0, 1).toUpperCase()}
                  avatarClass={avatarColorClass(skill.id)}
                  statusLabel={formatResourceStatus(skill.status, t)}
                  statusActive={skill.status === 'active'}
                  metaBadge={{
                    label: skill.source,
                    className:
                      'border border-border/60 bg-muted/60 font-mono uppercase tracking-wider text-muted-foreground',
                  }}
                  description={
                    skill.description || skill.body_markdown?.slice(0, 100) || t('common:state.noDescription')
                  }
                  stats={[
                    fileCount > 0
                      ? { key: 'files', icon: FileCode2, content: fileCount }
                      : { key: 'nomd', content: 'SKILL.md' },
                  ]}
                />
              )
            })}
            {filtered.length === 0 ? (
              <NoMatchesState message={t('skills:noMatches', '没有匹配的技能。')} />
            ) : null}
          </div>
        </div>
      )}
    </DetailShell>
  )
}
