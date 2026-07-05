import { useNavigate, useParams } from 'react-router-dom'

import { SkillResourcesPanel } from '@/components/skills/SkillResourcesPanel'
import { Button } from '@/components/ui/button'
import { useDeleteSkill, useSkill } from '@/hooks/useSkills'

export function SkillDetailRightPane() {
  const { skillId } = useParams<{ skillId: string }>()
  const skill = useSkill(skillId)
  const del = useDeleteSkill()
  const navigate = useNavigate()

  if (skill.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }
  if (skill.error) {
    return (
      <div className="p-6 text-sm text-destructive">
        Failed to load: {String(skill.error)}
      </div>
    )
  }
  if (!skill.data) {
    return <div className="p-6 text-sm text-muted-foreground">Skill not found.</div>
  }

  const s = skill.data

  const onDelete = async () => {
    if (!confirm(`Delete skill "${s.name}"? Mounted agents will lose this fragment.`)) {
      return
    }
    await del.mutateAsync(s.id)
    void navigate('/skills')
  }

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-3xl space-y-6 p-8">
        <header className="flex items-baseline justify-between gap-4">
          <div className="space-y-1">
            <h1 className="font-serif text-xl font-semibold tracking-tight">{s.name}</h1>
            {s.description && (
              <p className="text-sm text-muted-foreground">{s.description}</p>
            )}
            <p className="text-[11px] uppercase tracking-wider text-muted-foreground">
              source: {s.source}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onDelete}
            disabled={del.isPending}
          >
            {del.isPending ? 'Deleting…' : 'Delete'}
          </Button>
        </header>

        <section className="space-y-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Body (rendered as appended system-prompt fragment)
          </h3>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 text-sm">
            {s.body_markdown}
          </pre>
        </section>

        <SkillResourcesPanel skill={s} />
      </div>
    </div>
  )
}
