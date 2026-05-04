import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useAgents } from '@/hooks/useAgents'
import { useCreateGroup } from '@/hooks/useCreateGroup'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  workspace_id: z.string().min(1, 'Select a workspace'),
  description: z.string().optional(),
  announcement: z.string().optional(),
})

type FormValues = z.infer<typeof schema>

interface CreateGroupFormProps {
  onCreated: (newGroupId: string) => void
}

export function CreateGroupForm({ onCreated }: CreateGroupFormProps) {
  const agents = useAgents()
  const workspaces = useWorkspaces()
  const createGroup = useCreateGroup()
  const [selectedAgentIds, setSelectedAgentIds] = useState<string[]>([])
  const [submitError, setSubmitError] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: '', workspace_id: '', description: '', announcement: '' },
  })

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
        workspace_id: values.workspace_id,
        description: values.description || null,
        announcement: values.announcement || null,
        initial_agents: selectedAgentIds.length ? selectedAgentIds : undefined,
      })
      form.reset()
      setSelectedAgentIds([])
      onCreated(created.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : 'Network error')
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="group-name">Group name</Label>
        <Input id="group-name" {...form.register('name')} />
        {form.formState.errors.name && (
          <p className="text-xs text-red-600">{form.formState.errors.name.message}</p>
        )}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="group-workspace">Workspace</Label>
        <select
          id="group-workspace"
          className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          {...form.register('workspace_id')}
        >
          <option value="">Select workspace</option>
          {(workspaces.data ?? []).map((workspace) => (
            <option key={workspace.id} value={workspace.id}>
              {workspace.name} ({workspace.backend_type})
            </option>
          ))}
        </select>
        {form.formState.errors.workspace_id ? (
          <p className="text-xs text-red-600">
            {form.formState.errors.workspace_id.message}
          </p>
        ) : null}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="group-description">Description (optional)</Label>
        <Input id="group-description" {...form.register('description')} />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="group-announcement">Announcement (optional)</Label>
        <Textarea
          id="group-announcement"
          rows={2}
          placeholder="A short statement included in every agent's system prompt."
          {...form.register('announcement')}
        />
      </div>
      <div className="space-y-1.5">
        <Label>Add agents to the group (optional)</Label>
        {agents.isLoading && (
          <p className="text-xs text-muted-foreground">Loading agents…</p>
        )}
        {agents.data && agents.data.length === 0 && (
          <p className="text-xs text-muted-foreground">
            You have no agents yet. Create one on the Agents page first.
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
                    className={
                      checked
                        ? 'rounded-md border border-primary bg-primary px-3 py-1 text-xs text-primary-foreground'
                        : 'rounded-md border border-border bg-background px-3 py-1 text-xs hover:bg-muted'
                    }
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
      <Button type="submit" disabled={createGroup.isPending || workspaces.isLoading}>
        {createGroup.isPending ? 'Creating…' : 'Create group'}
      </Button>
    </form>
  )
}
