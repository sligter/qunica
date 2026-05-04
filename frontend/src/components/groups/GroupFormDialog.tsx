import { useEffect, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { useNavigate } from 'react-router-dom'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useAgents } from '@/hooks/useAgents'
import { useCreateGroup } from '@/hooks/useCreateGroup'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  announcement: z.string().optional(),
  free_speech: z.boolean(),
  allow_agent_free_mention: z.boolean(),
})

type FormValues = z.infer<typeof schema>

interface GroupFormDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Modal dialog for creating a new group. Editing an existing group is handled
 * by `GroupSettingsDialog` (mounted on the chat header's gear icon), which
 * also covers muted/admin agent management — out of scope here.
 */
export function GroupFormDialog({ open, onOpenChange }: GroupFormDialogProps) {
  const navigate = useNavigate()
  const agents = useAgents()
  const createGroup = useCreateGroup()
  const [selectedAgentIds, setSelectedAgentIds] = useState<string[]>([])
  const [submitError, setSubmitError] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      description: '',
      announcement: '',
      free_speech: false,
      allow_agent_free_mention: true,
    },
  })

  // Reset form values when the dialog opens.
  useEffect(() => {
    if (open) {
      form.reset({
        name: '',
        description: '',
        announcement: '',
        free_speech: false,
        allow_agent_free_mention: true,
      })
      setSelectedAgentIds([])
      setSubmitError(null)
    }
  }, [open, form])

  const toggleAgent = (id: string) => {
    setSelectedAgentIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    )
  }

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      const created = await createGroup.mutateAsync({
        name: values.name,
        description: values.description ?? null,
        announcement: values.announcement ?? null,
        initial_agents: selectedAgentIds.length ? selectedAgentIds : undefined,
      })
      onOpenChange(false)
      void navigate(`/groups/${created.id}`)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : 'Network error')
    }
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Create a new group</DialogTitle>
          <DialogDescription>
            A group is the shared context where users and agents collaborate.
            Behavior toggles can be tweaked later from the group's settings.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={onSubmit} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="gd-name">Name</Label>
            <Input id="gd-name" {...form.register('name')} />
            {form.formState.errors.name && (
              <p className="text-xs text-red-600">
                {form.formState.errors.name.message}
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gd-description">Description (optional)</Label>
            <Input id="gd-description" {...form.register('description')} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gd-announcement">Announcement (optional)</Label>
            <Textarea
              id="gd-announcement"
              rows={2}
              placeholder="A short statement included in every agent's system prompt."
              {...form.register('announcement')}
            />
          </div>

          <div className="space-y-1.5">
            <Label>Initial agents (optional)</Label>
            {agents.data && agents.data.length === 0 && (
              <p className="text-xs text-muted-foreground">
                No agents yet. Create one in the Agents tab first.
              </p>
            )}
            {agents.data && agents.data.length > 0 && (
              <ul className="flex flex-wrap gap-2">
                {agents.data.map((a) => {
                  const checked = selectedAgentIds.includes(a.id)
                  return (
                    <li key={a.id}>
                      <button
                        type="button"
                        onClick={() => toggleAgent(a.id)}
                        className={cn(
                          'rounded-md border px-3 py-1 text-xs transition-colors',
                          checked
                            ? 'border-primary bg-primary text-primary-foreground'
                            : 'border-border bg-background hover:bg-muted',
                        )}
                      >
                        {a.name}
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
          </div>

          {submitError && (
            <p className="text-sm text-red-600" role="alert">
              {submitError}
            </p>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={createGroup.isPending}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={createGroup.isPending}>
              {createGroup.isPending ? 'Creating…' : 'Create group'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

