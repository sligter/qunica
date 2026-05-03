import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import { useUpdateAgent } from '@/hooks/useUpdateAgent'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type { AgentRead } from '@/types/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  system_prompt: z.string().min(1, 'Required'),
  llm_provider_id: z.string().optional(),
})

type FormValues = z.infer<typeof schema>

interface EditAgentFormProps {
  agent: AgentRead
  onSaved?: () => void
}

export function EditAgentForm({ agent, onSaved }: EditAgentFormProps) {
  const update = useUpdateAgent(agent.id)
  const providers = useProviders()
  const skills = useSkills()
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>(agent.skill_ids)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: agent.name,
      description: agent.description ?? '',
      system_prompt: agent.system_prompt,
      llm_provider_id: agent.llm_provider_id ?? '',
    },
  })

  const toggleSkill = (id: string) => {
    setSelectedSkillIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    )
  }

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      await update.mutateAsync({
        name: values.name,
        description: values.description ?? null,
        system_prompt: values.system_prompt,
        llm_provider_id: values.llm_provider_id || null,
        skill_ids: selectedSkillIds,
      })
      onSaved?.()
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : 'Network error')
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="ea-name">Name</Label>
        <Input id="ea-name" {...form.register('name')} />
        {form.formState.errors.name && (
          <p className="text-xs text-red-600">{form.formState.errors.name.message}</p>
        )}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="ea-description">Description (optional)</Label>
        <Input id="ea-description" {...form.register('description')} />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="ea-prompt">System prompt</Label>
        <Textarea id="ea-prompt" rows={6} {...form.register('system_prompt')} />
        {form.formState.errors.system_prompt && (
          <p className="text-xs text-red-600">
            {form.formState.errors.system_prompt.message}
          </p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="ea-provider">LLM provider</Label>
        <select
          id="ea-provider"
          className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          {...form.register('llm_provider_id')}
        >
          <option value="">Default (env settings)</option>
          {(providers.data ?? []).map((p) => (
            <option key={p.id} value={p.id}>
              {p.name} — {p.kind} · {p.default_model}
            </option>
          ))}
        </select>
      </div>

      <div className="space-y-1.5">
        <Label>Mounted skills</Label>
        {skills.data && skills.data.length === 0 && (
          <p className="text-[11px] text-muted-foreground">
            No skills available. Import one in <strong>Skills</strong>.
          </p>
        )}
        {skills.data && skills.data.length > 0 && (
          <ul className="flex flex-wrap gap-2">
            {skills.data.map((s) => {
              const checked = selectedSkillIds.includes(s.id)
              return (
                <li key={s.id}>
                  <button
                    type="button"
                    onClick={() => toggleSkill(s.id)}
                    className={cn(
                      'rounded-md border px-3 py-1 text-xs transition-colors',
                      checked
                        ? 'border-primary bg-primary text-primary-foreground'
                        : 'border-border bg-background hover:bg-muted',
                    )}
                  >
                    {s.name}
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
      <Button type="submit" disabled={update.isPending}>
        {update.isPending ? 'Saving…' : 'Save'}
      </Button>
    </form>
  )
}
