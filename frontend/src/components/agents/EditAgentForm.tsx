import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
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
  temperature: z.number().min(0).max(2).optional(),
  top_p: z.number().min(0).max(1).optional(),
  max_tokens: z.number().int().min(1).optional(),
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
  const [showAdvanced, setShowAdvanced] = useState(false)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: agent.name,
      description: agent.description ?? '',
      system_prompt: agent.system_prompt,
      llm_provider_id: agent.llm_provider_id ?? '',
      temperature: (agent.llm_config?.temperature as number) ?? 0.7,
      top_p: (agent.llm_config?.top_p as number) ?? 1,
      max_tokens: (agent.llm_config?.max_tokens as number) ?? undefined,
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
      const llm_config: Record<string, unknown> = {}
      if (values.temperature !== undefined) llm_config.temperature = values.temperature
      if (values.top_p !== undefined && values.top_p !== 1) llm_config.top_p = values.top_p
      if (values.max_tokens) llm_config.max_tokens = values.max_tokens

      await update.mutateAsync({
        name: values.name,
        description: values.description ?? null,
        system_prompt: values.system_prompt,
        llm_config: Object.keys(llm_config).length > 0 ? llm_config : null,
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
        <button
          type="button"
          className="flex items-center gap-1 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
          onClick={() => setShowAdvanced(!showAdvanced)}
        >
          {showAdvanced ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
          Model Parameters
        </button>
        {showAdvanced && (
          <div className="space-y-4 rounded-md border border-border bg-card p-4">
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label>Temperature</Label>
                <span className="text-xs text-muted-foreground">{form.watch('temperature')?.toFixed(1)}</span>
              </div>
              <Slider
                min={0}
                max={2}
                step={0.1}
                value={[form.watch('temperature') ?? 0.7]}
                onValueChange={([v]) => form.setValue('temperature', v)}
              />
            </div>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label>Top P</Label>
                <span className="text-xs text-muted-foreground">{form.watch('top_p')?.toFixed(2)}</span>
              </div>
              <Slider
                min={0}
                max={1}
                step={0.01}
                value={[form.watch('top_p') ?? 1]}
                onValueChange={([v]) => form.setValue('top_p', v)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="ea-max-tokens">Max tokens (optional)</Label>
              <Input
                id="ea-max-tokens"
                type="number"
                placeholder="e.g. 4096"
                {...form.register('max_tokens', { valueAsNumber: true })}
              />
            </div>
          </div>
        )}
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
