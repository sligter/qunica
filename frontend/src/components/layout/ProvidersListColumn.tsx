import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useProviders } from '@/hooks/useProviders'
import type { ProviderKind } from '@/types/api'

interface ProvidersListColumnProps {
  width?: number
}

function kindColor(kind: ProviderKind): string {
  if (kind === 'anthropic') return 'bg-avatar-1 text-avatar-foreground'
  if (kind === 'anthropic-compatible') return 'bg-avatar-2 text-avatar-foreground'
  if (kind === 'gemini') return 'bg-avatar-3 text-avatar-foreground'
  return 'bg-avatar-4 text-avatar-foreground'
}

function kindInitial(kind: ProviderKind, name: string): string {
  if (kind === 'anthropic') return 'A'
  if (kind === 'anthropic-compatible') return 'C'
  if (kind === 'gemini') return 'G'
  return name.slice(0, 1).toUpperCase()
}

export function ProvidersListColumn({ width }: ProvidersListColumnProps) {
  const { t } = useTranslation('providers')
  const providers = useProviders()

  return (
    <ListColumn
      title={t('title')}
      newTo="/providers/new"
      newLabel={t('new')}
      searchPlaceholder={t('search')}
      isLoading={providers.isLoading}
      loadError={!!providers.error}
      errorText={t('loadError')}
      emptyText={t('empty')}
      width={width}
      items={(providers.data ?? []).map((p) => ({
        id: p.id,
        to: `/providers/${p.id}`,
        name: p.name,
        summary: `${p.kind} · ${p.default_model}`,
        avatarClass: kindColor(p.kind),
        avatarInitial: kindInitial(p.kind, p.name),
      }))}
    />
  )
}
