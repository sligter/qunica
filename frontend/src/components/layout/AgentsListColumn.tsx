import { Bot } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useAgents } from '@/hooks/useAgents'
import { useDeleteAgent } from '@/hooks/useDeleteAgent'
import { useRenameResource } from '@/hooks/useRenameResource'
import { avatarColorClass } from '@/lib/avatarColor'

export function AgentsListColumn() {
  const { t } = useTranslation('agents')
  const agents = useAgents()
  const rename = useRenameResource('/agents', ['agents'])
  const del = useDeleteAgent()

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
        deleteTitle: t('detail.deleteTitle', { name: a.name }),
        deleteDescription: t('detail.deleteDescription'),
      }))}
      onRename={(item, name) => rename.mutateAsync({ id: item.id, name })}
      onDelete={(item) => del.mutateAsync(item.id)}
    />
  )
}
