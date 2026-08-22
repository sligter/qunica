import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { FileCode2, Plus, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { DetailShell } from '@/components/layout/DetailShell'
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
      <div className="space-y-6">
        {/* Metric Cards Row */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('skills:title')}</span>
              <Sparkles className="h-4 w-4 text-primary/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{list.length}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('common:state.active')}</span>
              <span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-success">{activeCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('skills:resources.title')}</span>
              <FileCode2 className="h-4 w-4 text-info/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-info">{totalFiles}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('skills:detail.sourceShort')}</span>
              <span className="font-mono text-[10px] uppercase text-muted-foreground">pkg</span>
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">
              {list.filter((s) => s.source !== 'manual').length}
            </p>
          </div>
        </div>

        {/* Gallery Grid or Empty State */}
        {list.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 bg-card/30 p-12 text-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary">
              <Sparkles className="h-6 w-6" />
            </div>
            <h3 className="text-base font-semibold">{t('skills:empty')}</h3>
            <p className="mt-1 max-w-sm text-sm text-muted-foreground">
              {t('skills:form.createSubtitle')}
            </p>
            <Button className="mt-6 gap-2" asChild>
              <Link to="/skills/new">
                <Plus className="h-4 w-4" />
                {t('skills:import')}
              </Link>
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-2 xl:grid-cols-3">
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
              <p className="col-span-full py-12 text-center text-sm text-muted-foreground">
                {t('skills:noMatches', '没有匹配的技能。')}
              </p>
            ) : null}
          </div>
        )}
      </div>
    </DetailShell>
  )
}
