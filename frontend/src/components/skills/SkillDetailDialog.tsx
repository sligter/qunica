import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import { useDeleteSkill } from '@/hooks/useSkills'
import type { SkillRead } from '@/types/api'

interface SkillDetailDialogProps {
  skill: SkillRead | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SkillDetailDialog({ skill, open, onOpenChange }: SkillDetailDialogProps) {
  const del = useDeleteSkill()

  if (!skill) return null

  const onDelete = async () => {
    if (!confirm(`Delete skill "${skill.name}"? Mounted agents will lose this fragment.`)) {
      return
    }
    await del.mutateAsync(skill.id)
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-center justify-between pr-8">
            <div className="space-y-1">
              <DialogTitle>{skill.name}</DialogTitle>
              {skill.description && (
                <p className="text-sm text-muted-foreground">{skill.description}</p>
              )}
            </div>
            <Button
              variant="destructive"
              size="sm"
              onClick={onDelete}
              disabled={del.isPending}
            >
              {del.isPending ? 'Deleting…' : 'Delete'}
            </Button>
          </div>
        </DialogHeader>

        <div className="flex items-center gap-2">
          <Badge variant="outline" className="text-[10px] uppercase">
            source: {skill.source}
          </Badge>
          <Badge variant={skill.status === 'active' ? 'default' : 'secondary'} className="text-[10px]">
            {skill.status}
          </Badge>
        </div>

        <Separator />

        <section className="space-y-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Body (rendered as appended system-prompt fragment)
          </h3>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 text-sm max-h-64 overflow-y-auto">
            {skill.body_markdown}
          </pre>
        </section>
      </DialogContent>
    </Dialog>
  )
}
