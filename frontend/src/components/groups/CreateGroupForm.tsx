import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { Link } from 'react-router-dom'
import { z } from 'zod'

import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useAgents } from '@/hooks/useAgents'
import { useCreateGroup } from '@/hooks/useCreateGroup'
import { useSystemSettings } from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api-v2/client'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  announcement: z.string().optional(),
})

type FormValues = z.infer<typeof schema>

interface CreateGroupFormProps {
  onCreated: (newGroupId: string) => void
}

export function CreateGroupForm({ onCreated }: CreateGroupFormProps) {
  const agents = useAgents()
  const settings = useSystemSettings()
  const createGroup = useCreateGroup()
  const [selectedAgentIds, setSelectedAgentIds] = useState<string[]>([])
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState('')
  const [submitError, setSubmitError] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: '', description: '', announcement: '' },
  })

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
        workspace_id: selectedWorkspaceId || undefined,
        description: values.description || null,
        announcement: values.announcement || null,
        initial_agents: selectedAgentIds.length ? selectedAgentIds : undefined,
      })
      form.reset()
      setSelectedAgentIds([])
      setSelectedWorkspaceId('')
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
      <div className="rounded-md border border-border bg-muted/30 p-3 text-xs">
        <p className="font-medium">Workspace</p>
        <p className="mb-2 text-muted-foreground">
          Choose an existing workspace or create a local one. Leave it empty to
          auto-create from the system root.
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
            A dedicated workspace will be auto-created under{' '}
            <code>{settings.data?.group_workspace_root}</code>.
          </p>
        ) : (
          <p className="text-red-600">
            Group workspace root is not configured.{' '}
            <Link className="underline" to="/settings/system">
              Set it in system settings
            </Link>{' '}
            before creating a group.
          </p>
        )}
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
      <Button
        type="submit"
        disabled={
          createGroup.isPending ||
          (!selectedWorkspaceId && (settings.isLoading || !rootConfigured))
        }
      >
        {createGroup.isPending ? 'Creating…' : 'Create group'}
      </Button>
    </form>
  )
}
