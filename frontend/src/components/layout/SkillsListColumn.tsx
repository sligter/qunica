import { ListColumn } from '@/components/layout/ListColumn'
import { useSkills } from '@/hooks/useSkills'
import { avatarColorClass } from '@/lib/avatarColor'

interface SkillsListColumnProps {
  width?: number
}

export function SkillsListColumn({ width }: SkillsListColumnProps) {
  const skills = useSkills()

  return (
    <ListColumn
      title="Skills"
      newTo="/settings/skills/new"
      newLabel="Import skill"
      searchPlaceholder="Search skills"
      isLoading={skills.isLoading}
      loadError={!!skills.error}
      errorText="Failed to load skills."
      emptyText="No skills yet. Click + to import one."
      width={width}
      items={(skills.data ?? []).map((s) => ({
        id: s.id,
        to: `/settings/skills/${s.id}`,
        name: s.name,
        summary: s.description || s.body_markdown.slice(0, 80),
        avatarClass: avatarColorClass(s.id),
        avatarInitial: s.name.slice(0, 1).toUpperCase(),
      }))}
    />
  )
}
