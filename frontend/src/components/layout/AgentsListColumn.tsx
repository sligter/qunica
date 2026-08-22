import { Bot } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useAgents } from '@/hooks/useAgents'
import { avatarColorClass } from '@/lib/avatarColor'

interface AgentsListColumnProps {
  width?: number
}

export function AgentsListColumn({ width }: AgentsListColumnProps) {
  const { t } = useTranslation('agents')
  const agents = useAgents()

  return (
    <ListColumn
      title={t('title')}
      newTo="/agents/new"
      newLabel={t('new')}
      searchPlaceholder={t('search')}
      isLoading={agents.isLoading}
      loadError={!!agents.error}
      errorText={t('loadError')}
      emptyText={t('empty')}
      icon={Bot}
      width={width}
      items={(agents.data ?? []).map((a) => ({
        id: a.id,
        to: `/agents/${a.id}`,
        name: a.name,
        summary:
          a.runtime_kind === 'acp'
            ? t('acpRuntime')
            : a.description || a.system_prompt,
        avatarClass: avatarColorClass(a.id),
        avatarInitial: a.name.slice(0, 1).toUpperCase(),
      }))}
    />
  )
}
