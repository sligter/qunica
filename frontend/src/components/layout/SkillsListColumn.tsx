import { Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useSkills } from '@/hooks/useSkills'
import { avatarColorClass } from '@/lib/avatarColor'

interface SkillsListColumnProps {
  width?: number
}

export function SkillsListColumn({ width }: SkillsListColumnProps) {
  const { t } = useTranslation('skills')
  const skills = useSkills()

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
      width={width}
      items={(skills.data ?? []).map((s) => ({
        id: s.id,
        to: `/skills/${s.id}`,
        name: s.name,
        summary: s.description || s.body_markdown.slice(0, 80),
        avatarClass: avatarColorClass(s.id),
        avatarInitial: s.name.slice(0, 1).toUpperCase(),
      }))}
    />
  )
}
