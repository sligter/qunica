import { useState } from 'react'

import { CreateAgentForm } from '@/components/agents/CreateAgentForm'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface CreateAgentDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CreateAgentDialog({ open, onOpenChange }: CreateAgentDialogProps) {
  const [, setCreatedId] = useState<string | null>(null)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>New Agent</DialogTitle>
          <DialogDescription>
            Define an agent's name, system prompt, and optional model parameters.
          </DialogDescription>
        </DialogHeader>
        <CreateAgentForm
          onCreated={(id) => {
            setCreatedId(id)
            onOpenChange(false)
          }}
        />
      </DialogContent>
    </Dialog>
  )
}
