import { useNavigate, useParams } from 'react-router-dom'
import { Plus } from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { useSkills } from '@/hooks/useSkills'
import { cn } from '@/lib/utils'

export function SkillsList() {
  const skills = useSkills()
  const { skillId: activeId } = useParams<{ skillId: string }>()
  const navigate = useNavigate()
  const isCreateView = !activeId

  return (
    <div className="flex h-full w-72 shrink-0 flex-col border-r border-border bg-background">
      <div className="flex h-14 items-center justify-between border-b border-border px-4">
        <h2 className="text-sm font-semibold">Skills</h2>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void navigate('/skills')}
          aria-label="New skill"
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        {skills.isLoading && (
          <p className="px-4 text-xs text-muted-foreground">Loading…</p>
        )}
        {skills.error && (
          <p className="px-4 text-xs text-destructive">Failed to load skills.</p>
        )}
        {skills.data && skills.data.length === 0 && (
          <p className="px-4 text-xs text-muted-foreground">
            No skills yet. Click + to import a SKILL.md.
          </p>
        )}

        {isCreateView && skills.data && skills.data.length > 0 && (
          <p className="mb-2 px-3 text-[10px] uppercase tracking-wider text-muted-foreground">
            Import a new skill on the right.
          </p>
        )}

        <ul className="space-y-0.5 px-2">
          {(skills.data ?? []).map((s) => {
            const isActive = s.id === activeId
            return (
              <li key={s.id}>
                <button
                  type="button"
                  onClick={() => void navigate(`/skills/${s.id}`)}
                  className={cn(
                    'flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                    isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                  )}
                >
                  <Avatar className="h-9 w-9 shrink-0">
                    <AvatarFallback className="bg-avatar-2 text-avatar-foreground">
                      {s.name.slice(0, 1).toUpperCase()}
                    </AvatarFallback>
                  </Avatar>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span
                      className={cn(
                        'truncate text-sm',
                        isActive ? 'font-semibold' : 'font-medium',
                      )}
                    >
                      {s.name}
                    </span>
                    <p className="line-clamp-1 text-xs text-muted-foreground">
                      {s.description || s.body_markdown.slice(0, 80)}
                    </p>
                  </div>
                </button>
              </li>
            )
          })}
        </ul>
      </div>
    </div>
  )
}
