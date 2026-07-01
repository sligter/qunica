import { useEffect, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { Link, useNavigate } from 'react-router-dom'
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
import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { useAgents } from '@/hooks/useAgents'
import { useCreateGroup } from '@/hooks/useCreateGroup'
import { useSystemSettings } from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/http'
import { cn } from '@/lib/utils'
import type { GroupCommunicationMode } from '@/types/api'

const communicationModeOptions: Array<{
  value: GroupCommunicationMode
  label: string
  description: string
}> = [
  {
    value: 'mesh',
    label: 'Mesh',
    description: 'Peer collaboration for creative or dynamic work.',
  },
  {
    value: 'star',
    label: 'Star',
    description: 'Admin hub speaks first, then other routed agents.',
  },
  {
    value: 'hierarchical',
    label: 'Hierarchical',
    description: 'Admin agents lead before worker agents.',
  },
  {
    value: 'ring',
    label: 'Ring',
    description: 'Agents take turns in a stable pipeline order.',
  },
]

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  announcement: z.string().optional(),
  communication_mode: z.enum(['mesh', 'star', 'hierarchical', 'ring']),
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
  const settings = useSystemSettings()
  const createGroup = useCreateGroup()
  const [selectedAgentIds, setSelectedAgentIds] = useState<string[]>([])
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState('')
  const [submitError, setSubmitError] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      description: '',
      announcement: '',
      communication_mode: 'mesh',
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
        communication_mode: 'mesh',
        free_speech: false,
        allow_agent_free_mention: true,
      })
      setSelectedAgentIds([])
      setSelectedWorkspaceId('')
      setSubmitError(null)
    }
  }, [open, form])

  const toggleAgent = (id: string) => {
    setSelectedAgentIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    )
  }

  const rootConfigured = Boolean(settings.data?.group_workspace_root)

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      const created = await createGroup.mutateAsync({
        name: values.name,
        description: values.description ?? null,
        announcement: values.announcement ?? null,
        communication_mode: values.communication_mode,
        workspace_id: selectedWorkspaceId || undefined,
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
      <DialogContent className="flex max-h-[85vh] w-[95vw] flex-col gap-4 overflow-hidden sm:max-w-2xl">
        <DialogHeader className="shrink-0">
          <DialogTitle>Create a new group</DialogTitle>
          <DialogDescription>
            A group is the shared context where users and agents collaborate.
            Choose an existing workspace, create a local workspace, or let the
            app auto-create one under your configured group workspace root.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={onSubmit} className="flex min-h-0 flex-1 flex-col gap-4">
          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
          <div className="space-y-1.5">
            <Label htmlFor="gd-name">Name</Label>
            <Input id="gd-name" {...form.register('name')} />
            {form.formState.errors.name && (
              <p className="text-xs text-red-600">
                {form.formState.errors.name.message}
              </p>
            )}
          </div>

          <div className="rounded-md border border-border bg-muted/30 p-3 text-xs">
            <p className="font-medium">Group workspace</p>
            <p className="mb-2 text-muted-foreground">
              Choose an existing workspace or create a local one. Leave it empty
              to auto-create from the system root.
            </p>
            <WorkspaceField value={selectedWorkspaceId} onChange={setSelectedWorkspaceId} />
            {selectedWorkspaceId ? (
              <p className="mt-2 text-muted-foreground">
                The selected workspace will be used for this group.
              </p>
            ) : settings.isLoading ? (
              <p className="text-muted-foreground">Loading system settings…</p>
            ) : rootConfigured ? (
              <p className="text-muted-foreground">
                A new dedicated workspace will be created under{' '}
                <code>{settings.data?.group_workspace_root}</code>.
              </p>
            ) : (
              <p className="text-red-600">
                Group workspace root is not configured.{' '}
                <Link
                  className="underline"
                  to="/settings/system"
                  onClick={() => onOpenChange(false)}
                >
                  Set it in system settings
                </Link>{' '}
                before creating a group.
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
            <Label htmlFor="gd-communication-mode">Communication mode</Label>
            <select
              id="gd-communication-mode"
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-1 focus:ring-ring"
              {...form.register('communication_mode')}
            >
              {communicationModeOptions.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <p className="text-xs text-muted-foreground">
              {
                communicationModeOptions.find(
                  (option) => option.value === form.watch('communication_mode'),
                )?.description
              }
            </p>
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

          </div>

          <DialogFooter className="shrink-0">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={createGroup.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={
                createGroup.isPending ||
                (!selectedWorkspaceId && (settings.isLoading || !rootConfigured))
              }
            >
              {createGroup.isPending ? 'Creating…' : 'Create group'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
