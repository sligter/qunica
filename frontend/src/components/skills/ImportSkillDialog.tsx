import { ImportSkillForm } from '@/components/skills/ImportSkillForm'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface ImportSkillDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function ImportSkillDialog({ open, onOpenChange }: ImportSkillDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Import Skill</DialogTitle>
          <DialogDescription>
            Upload a skill package, paste a SKILL.md file, or install from GitHub.
            The skill body will be appended to an agent's system prompt when mounted.
          </DialogDescription>
        </DialogHeader>
        <ImportSkillForm onCreated={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}
