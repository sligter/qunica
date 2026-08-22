import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Layers, Plug, Plus, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { SearchInput } from '@/components/ui/search-input'
import { useProviders } from '@/hooks/useProviders'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import type { ProviderKind } from '@/types/api'

function kindBadgeClass(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') {
    return 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/20'
  }
  if (kind === 'gemini') {
    return 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20'
  }
  return 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20'
}

export function ProvidersIndexPage() {
  const { t } = useTranslation(['providers', 'common'])
  const providers = useProviders()
  const [query, setQuery] = useState('')

  const list = providers.data ?? []
  const listKey = list.map((p) => p.id).join(',')
  // Model IDs are how people look providers up — "which one serves glm-5.2"
  // is a search over default_model, not over names.
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return list
    return list.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.default_model?.toLowerCase().includes(q),
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- list is stable per query cache entry; keying on ids avoids re-running the filter on unrelated renders
  }, [listKey, query])
  const activeCount = list.filter((p) => p.status === 'active').length
  const totalModels = list.reduce(
    (acc, p) => acc + (p.models?.length ?? (p.default_model ? 1 : 0)),
    0,
  )

  return (
    <DetailShell
      title={t('providers:list.selectTitle')}
      subtitle={t('providers:list.selectDescription')}
      measure="wide"
      actions={
        <>
          {list.length > 0 ? (
            <SearchInput value={query} onChange={setQuery} label={t('providers:search')} />
          ) : null}
          <Button size="sm" asChild>
            <Link to="/providers/new">
              <Plus className="mr-1.5 h-3.5 w-3.5" />
              {t('providers:new')}
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
              <span className="text-xs font-medium text-muted-foreground">{t('providers:title')}</span>
              <Plug className="h-4 w-4 text-primary/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{list.length}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('providers:fields.status')}</span>
              <span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-success">{activeCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('providers:models.title')}</span>
              <Layers className="h-4 w-4 text-info/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-info">{totalModels}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('providers:fields.apiKey')}</span>
              <ShieldCheck className="h-4 w-4 text-emerald-500/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-foreground">{list.length}</p>
          </div>
        </div>

        {/* Gallery Grid or Empty State */}
        {list.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 bg-card/30 p-12 text-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary">
              <Plug className="h-6 w-6" />
            </div>
            <h3 className="text-base font-semibold">{t('providers:empty')}</h3>
            <p className="mt-1 max-w-sm text-sm text-muted-foreground">
              {t('providers:form.createSubtitle')}
            </p>
            <Button className="mt-6 gap-2" asChild>
              <Link to="/providers/new">
                <Plus className="h-4 w-4" />
                {t('providers:new')}
              </Link>
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-2 xl:grid-cols-3">
            {filtered.map((provider) => {
              const modelList = provider.models ?? []
              return (
                <EntityCard
                  key={provider.id}
                  to={`/providers/${provider.id}`}
                  editTo={`/providers/${provider.id}?edit=1`}
                  title={provider.name}
                  avatarIcon={<Plug className="h-5 w-5" />}
                  avatarClass="bg-primary/10 text-primary"
                  statusLabel={formatResourceStatus(provider.status, t)}
                  statusActive={provider.status === 'active'}
                  metaBadge={{ label: provider.kind, className: kindBadgeClass(provider.kind) }}
                  description={
                    <span className="flex items-center gap-1.5">
                      <span className="font-medium text-foreground/80">
                        {t('providers:fields.defaultModel')}:
                      </span>
                      <code className="font-mono text-2xs">{provider.default_model}</code>
                    </span>
                  }
                  stats={[
                    {
                      key: 'models',
                      icon: Layers,
                      content:
                        modelList.length > 0
                          ? `${modelList.length} ${t('providers:models.title')}`
                          : '1 model',
                    },
                  ]}
                />
              )
            })}
            {filtered.length === 0 ? (
              <p className="col-span-full py-12 text-center text-sm text-muted-foreground">
                {t('providers:noMatches', '没有匹配的服务商。')}
              </p>
            ) : null}
          </div>
        )}
      </div>
    </DetailShell>
  )
}
