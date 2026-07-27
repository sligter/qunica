import { useTranslation } from 'react-i18next'

import { EntityPicker } from '@/components/ui/entity-picker'
import { PageState } from '@/components/ui/page-state'
import type { SkillRead } from '@/types/api'

interface SkillPickerProps {
  skills: SkillRead[]
  isLoading?: boolean
  selectedIds: string[]
  onChange: (nextIds: string[]) => void
  /** Empty-state copy differs between creating and editing an agent. */
  emptyText: string
}

/**
 * The skills a agent mounts, shared by the create and edit forms.
 *
 * Both forms rendered the same list markup before; keeping it in one place is
 * what stops the two drifting apart the next time either changes.
 */
export function SkillPicker({
  skills,
  isLoading = false,
  selectedIds,
  onChange,
  emptyText,
}: SkillPickerProps) {
  const { t } = useTranslation('agents')

  if (isLoading) {
    return (
      <p className="text-xs text-muted-foreground">{t('states.loadingSkills')}</p>
    )
  }

  return (
    <EntityPicker
      label={t('fields.skills')}
      searchPlaceholder={t('form.searchSkills')}
      items={skills.map((skill) => ({
        id: skill.id,
        label: skill.name,
        meta: skill.description ?? undefined,
      }))}
      selectedIds={selectedIds}
      onChange={onChange}
      countLabel={(total, selected) =>
        t('form.skillCount', { total, selected, count: total })
      }
      empty={<PageState inset icon={null} title={emptyText} />}
    />
  )
}
