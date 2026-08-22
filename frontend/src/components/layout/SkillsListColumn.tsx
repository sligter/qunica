import { Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useDeleteSkill, useSkills } from '@/hooks/useSkills'
import { useRenameResource } from '@/hooks/useRenameResource'
import { avatarColorClass } from '@/lib/avatarColor'

export function SkillsListColumn() {
  const { t } = useTranslation('skills')
  const skills = useSkills()
  const rename = useRenameResource('/skills', ['skills'])
  const del = useDeleteSkill()

  return (
    <ListColumn
      title={t('title')}
      newTo="/skills/new"
      newLabel={t('import')}
      searchPlaceholder={t('search')}
      isLoading={skills.isLoading}
      loadError={!!skills.error}
      errorText={t('loadError')}
      emptyText={t('empty')}
      icon={Sparkles}
      items={(skills.data ?? []).map((s) => ({
        id: s.id,
        to: `/skills/${s.id}`,
        name: s.name,
        summary: s.description || s.body_markdown.slice(0, 80),
        avatarClass: avatarColorClass(s.id),
        avatarInitial: s.name.slice(0, 1).toUpperCase(),
        deleteTitle: t('detail.deleteTitle', { name: s.name }),
        deleteDescription: t('detail.deleteDescription'),
      }))}
      onRename={(item, name) => rename.mutateAsync({ id: item.id, name })}
      onDelete={(item) => del.mutateAsync(item.id)}
    />
  )
}
