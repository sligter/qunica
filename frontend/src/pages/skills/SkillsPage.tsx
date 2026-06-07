import { useState } from 'react'
import { Plus, Sparkles } from 'lucide-react'

import { ImportSkillDialog } from '@/components/skills/ImportSkillDialog'
import { SkillDetailDialog } from '@/components/skills/SkillDetailDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { useSkills } from '@/hooks/useSkills'
import type { SkillRead } from '@/types/api'

export function SkillsPage() {
  const skills = useSkills()
  const [importOpen, setImportOpen] = useState(false)
  const [selected, setSelected] = useState<SkillRead | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-6">
        <div className="flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-muted-foreground" />
          <h1 className="text-base font-semibold tracking-tight">Skills</h1>
          {skills.data && (
            <span className="text-xs text-muted-foreground">({skills.data.length})</span>
          )}
        </div>
        <Button size="sm" onClick={() => setImportOpen(true)}>
          <Plus className="mr-1 h-4 w-4" />
          Import Skill
        </Button>
      </header>

      <div className="flex-1 overflow-y-auto p-6">
        {skills.isLoading && (
          <p className="text-sm text-muted-foreground">Loading skills…</p>
        )}
        {skills.error && (
          <p className="text-sm text-red-600">Failed to load skills.</p>
        )}
        {skills.data && skills.data.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-3 py-20 text-center">
            <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <Sparkles className="h-7 w-7" />
            </div>
            <h2 className="text-base font-medium">No skills yet</h2>
            <p className="max-w-sm text-sm text-muted-foreground">
              Import a skill package, GitHub repository, or SKILL.md to extend your agents'
              capabilities.
            </p>
            <Button size="sm" onClick={() => setImportOpen(true)}>
              <Plus className="mr-1 h-4 w-4" />
              Import Skill
            </Button>
          </div>
        )}

        {skills.data && skills.data.length > 0 && (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {skills.data.map((s) => (
              <Card
                key={s.id}
                className="cursor-pointer transition-shadow hover:shadow-md"
                onClick={() => {
                  setSelected(s)
                  setDetailOpen(true)
                }}
              >
                <CardHeader className="flex flex-row items-start gap-3 space-y-0 pb-3">
                  <Avatar className="h-10 w-10 shrink-0">
                    <AvatarFallback className="bg-amber-500/90 text-white font-semibold">
                      {s.name.slice(0, 1).toUpperCase()}
                    </AvatarFallback>
                  </Avatar>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <h3 className="truncate text-sm font-semibold">{s.name}</h3>
                    <p className="line-clamp-2 text-xs text-muted-foreground">
                      {s.description || s.body_markdown.slice(0, 80)}
                    </p>
                  </div>
                </CardHeader>
                <CardContent>
                  <div className="flex flex-wrap gap-1.5">
                    <Badge variant="outline" className="text-[10px] uppercase">
                      {s.source}
                    </Badge>
                    <Badge
                      variant={s.status === 'active' ? 'default' : 'secondary'}
                      className="text-[10px]"
                    >
                      {s.status}
                    </Badge>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </div>

      <ImportSkillDialog open={importOpen} onOpenChange={setImportOpen} />
      <SkillDetailDialog
        skill={selected}
        open={detailOpen}
        onOpenChange={(v) => {
          setDetailOpen(v)
          if (!v) setSelected(null)
        }}
      />
    </div>
  )
}
