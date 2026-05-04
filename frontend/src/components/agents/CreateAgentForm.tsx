import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { z } from 'zod'

import { ToolSelector } from '@/components/agents/ToolSelector'
import { createDefaultToolConfig } from '@/components/agents/toolConfig'
import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { Textarea } from '@/components/ui/textarea'
import { useBuiltinTools } from '@/hooks/useBuiltinTools'
import { useCreateAgent } from '@/hooks/useCreateAgent'
import { useProviders } from '@/hooks/useProviders'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { useSkills } from '@/hooks/useSkills'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type { AgentToolConfig } from '@/types/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  system_prompt: z.string().min(1, 'Required'),
  llm_provider_id: z.string().optional(),
  workspace_id: z.string().min(1, 'Workspace is required'),
  temperature: z.number().min(0).max(2).optional(),
  top_p: z.number().min(0).max(1).optional(),
  max_tokens: z.number().int().min(1).optional(),
})

type FormValues = z.infer<typeof schema>

interface CreateAgentFormProps {
  onCreated?: (newAgentId: string) => void
}

export function CreateAgentForm({ onCreated }: CreateAgentFormProps = {}) {
  const createAgent = useCreateAgent()
  const providers = useProviders()
  const skills = useSkills()
  const workspaces = useWorkspaces()
  const builtinTools = useBuiltinTools()
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [submittedName, setSubmittedName] = useState<string | null>(null)
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>([])
  const [toolConfig, setToolConfig] = useState<AgentToolConfig | null>(null)
  const [showAdvanced, setShowAdvanced] = useState(false)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      description: '',
      system_prompt: '',
      llm_provider_id: '',
      workspace_id: '',
      temperature: 0.7,
      top_p: 1,
      max_tokens: undefined,
    },
  })

  const toggleSkill = (id: string) => {
    setSelectedSkillIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    )
  }

  const tools = builtinTools.data?.tools ?? []
  const selectedWorkspace = (workspaces.data ?? []).find(
    (workspace) => workspace.id === form.watch('workspace_id'),
  )
  const currentToolConfig = toolConfig ?? createDefaultToolConfig(tools)

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    setSubmittedName(null)
    try {
      const llm_config: Record<string, unknown> = {}
      if (values.temperature !== undefined) llm_config.temperature = values.temperature
      if (values.top_p !== undefined && values.top_p !== 1) llm_config.top_p = values.top_p
      if (values.max_tokens) llm_config.max_tokens = values.max_tokens

      const created = await createAgent.mutateAsync({
        name: values.name,
        description: values.description,
        system_prompt: values.system_prompt,
        llm_config: Object.keys(llm_config).length > 0 ? llm_config : null,
        tool_config: currentToolConfig,
        workspace_id: values.workspace_id,
        llm_provider_id: values.llm_provider_id || null,
        skill_ids: selectedSkillIds,
      })
      form.reset()
      setSelectedSkillIds([])
      setToolConfig(null)
      setSubmittedName(created.name)
      onCreated?.(created.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : 'Network error')
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="agent-name">Name</Label>
        <Input id="agent-name" placeholder="Echo" {...form.register('name')} />
        {form.formState.errors.name && (
          <p className="text-xs text-red-600">{form.formState.errors.name.message}</p>
        )}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="agent-description">Description (optional)</Label>
        <Input
          id="agent-description"
          placeholder="What this agent is for"
          {...form.register('description')}
        />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="agent-system-prompt">System prompt</Label>
        <Textarea
          id="agent-system-prompt"
          rows={5}
          placeholder="You are a concise assistant. Always end with the word DONE."
          {...form.register('system_prompt')}
        />
        {form.formState.errors.system_prompt && (
          <p className="text-xs text-red-600">
            {form.formState.errors.system_prompt.message}
          </p>
        )}
      </div>

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">Workspace</h3>
          <p className="text-[11px] text-muted-foreground">
            Bind this agent to a backend-visible project folder.
          </p>
        </div>
        <WorkspaceField
          value={form.watch('workspace_id')}
          onChange={(workspaceId) => form.setValue('workspace_id', workspaceId, { shouldValidate: true })}
          error={form.formState.errors.workspace_id?.message}
        />
      </section>

      <div className="space-y-1.5">
        <Label htmlFor="agent-provider">LLM provider</Label>
        <select
          id="agent-provider"
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
        {providers.data && providers.data.length === 0 && (
          <p className="text-[11px] text-muted-foreground">
            No providers registered. Go to <strong>Providers</strong> to add one,
            or leave this as Default.
          </p>
        )}
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
              <Label htmlFor="agent-max-tokens">Max tokens (optional)</Label>
              <Input
                id="agent-max-tokens"
                type="number"
                placeholder="e.g. 4096"
                {...form.register('max_tokens', { valueAsNumber: true })}
              />
            </div>
          </div>
        )}
      </div>

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">Built-in tools</h3>
          <p className="text-[11px] text-muted-foreground">
            Select tool permissions to include in the agent context. Risky tools are saved only and are not executed by this runtime.
          </p>
        </div>
        {builtinTools.isLoading && <p className="text-xs text-muted-foreground">Loading tools…</p>}
        {tools.length > 0 && (
          <ToolSelector
            tools={tools}
            value={currentToolConfig}
            workspaceBackendType={selectedWorkspace?.backend_type ?? 'local'}
            onChange={setToolConfig}
          />
        )}
      </section>

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">Skills</h3>
          <p className="text-[11px] text-muted-foreground">
            Enabled skills are mounted into the prompt through <code>skill_ids</code>.
          </p>
        </div>
        {skills.isLoading && (
          <p className="text-xs text-muted-foreground">Loading…</p>
        )}
        {skills.data && skills.data.length === 0 && (
          <p className="text-[11px] text-muted-foreground">
            No skills yet. Go to <strong>Skills</strong> to import a SKILL.md.
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
                    title={s.description ?? undefined}
                    className={cn(
                      'rounded-md border px-3 py-1 text-left text-xs transition-colors',
                      checked
                        ? 'border-primary bg-primary text-primary-foreground'
                        : 'border-border bg-background hover:bg-muted',
                    )}
                  >
                    <span className="block font-medium">{s.name}</span>
                    {s.description && (
                      <span className="block max-w-48 truncate opacity-75">{s.description}</span>
                    )}
                  </button>
                </li>
              )
            })}
          </ul>
        )}
      </section>

      {submitError && (
        <p className="text-sm text-red-600" role="alert">
          {submitError}
        </p>
      )}
      {submittedName && (
        <p className="text-sm text-green-700">Created agent: {submittedName}</p>
      )}
      <Button type="submit" disabled={createAgent.isPending}>
        {createAgent.isPending ? 'Creating…' : 'Create agent'}
      </Button>
    </form>
  )
}
