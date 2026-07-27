import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityMultiSelect } from '@/components/ui/entity-multi-select'
import { useSkills } from '@/hooks/useSkills'

interface SkillSelectorFieldProps {
  selectedIds: string[]
  onChange: (next: string[]) => void
  /** Copy shown when the catalog is empty; differs between create and edit. */
  emptyText: string
}

export function SkillSelectorField({ selectedIds, onChange, emptyText }: SkillSelectorFieldProps) {
  const { t } = useTranslation('agents')
  const skills = useSkills()
  const items = useMemo(
    () =>
      (skills.data ?? []).map((skill) => ({
        id: skill.id,
        name: skill.name,
        description: skill.description,
        keywords: [skill.source, skill.id],
      })),
    [skills.data],
  )

  return (
    <section className="space-y-2 rounded-md border border-border bg-card p-3">
      <div>
        <h3 className="text-sm font-medium">{t('fields.skills')}</h3>
        <p className="text-[11px] text-muted-foreground">{t('form.skillsDescription')}</p>
      </div>
      {skills.isLoading ? (
        <p className="text-xs text-muted-foreground">{t('states.loadingSkills')}</p>
      ) : (
        <EntityMultiSelect
          id="agent-skills"
          items={items}
          selectedIds={selectedIds}
          onChange={onChange}
          label={t('fields.skills')}
          searchPlaceholder={t('form.searchSkills')}
          emptyText={emptyText}
        />
      )}
    </section>
  )
}
