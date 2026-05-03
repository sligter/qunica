import { CreateProviderForm } from '@/components/providers/CreateProviderForm'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface CreateProviderDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CreateProviderDialog({ open, onOpenChange }: CreateProviderDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>New LLM Provider</DialogTitle>
          <DialogDescription>
            Register a chat-completion endpoint. The API key is stored securely
            and shown masked on the detail page.
          </DialogDescription>
        </DialogHeader>
        <CreateProviderForm onCreated={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}
