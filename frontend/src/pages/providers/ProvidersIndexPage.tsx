import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Layers, Plug, Plus, ShieldCheck } from 'lucide-react'
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
import { useProviders } from '@/hooks/useProviders'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { TINTED_BADGE } from '@/lib/tintedBadge'
import type { ProviderKind } from '@/types/api'

function kindBadgeClass(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') return TINTED_BADGE.amber
  if (kind === 'gemini') return TINTED_BADGE.blue
  return TINTED_BADGE.green
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
      {providers.isLoading ? (
        <EntityIndexSkeleton />
      ) : providers.error ? (
        <IndexErrorState
          title={t('providers:loadError')}
          detail={providers.error instanceof Error ? providers.error.message : undefined}
          onRetry={() => void providers.refetch()}
          retryLabel={t('common:actions.retry')}
        />
      ) : list.length === 0 ? (
        <EntityEmptyState
          icon={Plug}
          title={t('providers:empty')}
          description={t('providers:form.createSubtitle')}
          actionLabel={
            <>
              <Plus className="h-4 w-4" />
              {t('providers:new')}
            </>
          }
          actionTo="/providers/new"
        />
      ) : (
        <div className="space-y-6">
        {/* Metric Cards Row */}
        <MetricRow>
          <MetricCard label={t('providers:title')} value={list.length} icon={Plug} tone="primary" />
          <MetricCard
            label={t('providers:fields.status')}
            value={activeCount}
            tone="success"
            marker={<span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />}
          />
          <MetricCard
            label={t('providers:models.title')}
            value={totalModels}
            icon={Layers}
            tone="info"
          />
          <MetricCard
            label={t('providers:fields.apiKey')}
            value={list.length}
            icon={ShieldCheck}
            tone="success"
          />
        </MetricRow>

          {/* Gallery Grid */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
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
              <NoMatchesState message={t('providers:noMatches', '没有匹配的服务商。')} />
            ) : null}
          </div>
        </div>
      )}
    </DetailShell>
  )
}
