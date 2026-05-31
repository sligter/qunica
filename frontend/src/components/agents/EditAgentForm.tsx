import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { z } from 'zod'

import { ExternalRuntimeFields } from '@/components/agents/ExternalRuntimeFields'
import { SystemPromptMentionTextarea } from '@/components/agents/SystemPromptMentionTextarea'
import { ToolSelector } from '@/components/agents/ToolSelector'
import { mergeToolConfig } from '@/components/agents/toolConfig'
import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { useAgents } from '@/hooks/useAgents'
import { useBuiltinTools } from '@/hooks/useBuiltinTools'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import { useUpdateAgent } from '@/hooks/useUpdateAgent'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type {
  AgentRead,
  AgentRuntimeKind,
  AgentToolConfig,
  ExternalRuntimeAdapter,
} from '@/types/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  system_prompt: z.string().min(1, 'Required'),
  runtime_kind: z.enum(['llm_chat', 'external_cli']),
  external_adapter: z.enum(['codex', 'claude_code']),
  external_executable: z.string().optional(),
  external_timeout_seconds: z.number().int().min(1).max(21600),
  external_max_turns: z.number().int().min(1).max(100),
  llm_provider_id: z.string().optional(),
  workspace_id: z.string().min(1, 'Workspace is required'),
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
  const workspaces = useWorkspaces()
  const builtinTools = useBuiltinTools()
  const agents = useAgents()
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>(agent.skill_ids)
  const [toolConfig, setToolConfig] = useState<AgentToolConfig | null>(agent.tool_config)
  const [showAdvanced, setShowAdvanced] = useState(false)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: agent.name,
      description: agent.description ?? '',
      system_prompt: agent.system_prompt,
      runtime_kind: agent.runtime_kind ?? 'llm_chat',
      external_adapter: agent.external_runtime?.adapter ?? 'codex',
      external_executable: agent.external_runtime?.executable ?? '',
      external_timeout_seconds: agent.external_runtime?.timeout_seconds ?? 3600,
      external_max_turns: agent.external_runtime?.max_turns ?? 20,
      llm_provider_id: agent.llm_provider_id ?? '',
      workspace_id: agent.workspace_id ?? '',
      temperature: (agent.llm_config?.temperature as number) ?? 0.7,
      top_p: (agent.llm_config?.top_p as number) ?? 1,
      max_tokens: (agent.llm_config?.max_tokens as number) ?? undefined,
    },
  })

  const runtimeKind = form.watch('runtime_kind')
  const tools = builtinTools.data?.tools ?? []
  const selectedWorkspace = (workspaces.data ?? []).find(
    (workspace) => workspace.id === form.watch('workspace_id'),
  )
  const currentToolConfig = mergeToolConfig(tools, toolConfig)
  const systemPromptField = form.register('system_prompt')

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
        runtime_kind: values.runtime_kind,
        external_runtime:
          values.runtime_kind === 'external_cli'
            ? {
                adapter: values.external_adapter,
                executable: values.external_executable || null,
                timeout_seconds: values.external_timeout_seconds,
                max_turns: values.external_max_turns,
              }
            : null,
        llm_config:
          values.runtime_kind === 'llm_chat' && Object.keys(llm_config).length > 0
            ? llm_config
            : null,
        tool_config: values.runtime_kind === 'llm_chat' ? currentToolConfig : null,
        workspace_id: values.workspace_id,
        llm_provider_id:
          values.runtime_kind === 'llm_chat' ? values.llm_provider_id || null : null,
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
        <SystemPromptMentionTextarea
          id="ea-prompt"
          rows={6}
          value={form.watch('system_prompt')}
          onChange={(value) =>
            form.setValue('system_prompt', value, {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
          onBlur={systemPromptField.onBlur}
          name={systemPromptField.name}
          inputRef={systemPromptField.ref}
        />
        {form.formState.errors.system_prompt && (
          <p className="text-xs text-red-600">
            {form.formState.errors.system_prompt.message}
          </p>
        )}
      </div>

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">Runtime</h3>
          <p className="text-[11px] text-muted-foreground">
            Choose the execution engine for this agent.
          </p>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          {([
            ['llm_chat', 'LLM chat', 'Provider-native model and tools'],
            ['external_cli', 'External CLI', 'Codex or Claude Code'],
          ] as const).map(([value, label, hint]) => {
            const checked = runtimeKind === value
            return (
              <button
                key={value}
                type="button"
                onClick={() => form.setValue('runtime_kind', value as AgentRuntimeKind)}
                className={cn(
                  'rounded-md border px-3 py-2 text-left transition-colors',
                  checked
                    ? 'border-primary bg-primary/10'
                    : 'border-border bg-background hover:bg-muted',
                )}
              >
                <span className="block text-sm font-medium">{label}</span>
                <span className="block text-[11px] text-muted-foreground">{hint}</span>
              </button>
            )
          })}
        </div>
      </section>

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">Workspace</h3>
          <p className="text-[11px] text-muted-foreground">
            Bind this agent to a backend-visible project folder.
          </p>
        </div>
        <WorkspaceField
          value={form.watch('workspace_id')}
          onChange={(workspaceId) =>
            form.setValue('workspace_id', workspaceId, { shouldValidate: true })
          }
          error={form.formState.errors.workspace_id?.message}
        />
        {!agent.workspace_id && (
          <p className="text-xs text-amber-700">
            This existing agent has no workspace yet. Select one before saving.
          </p>
        )}
      </section>

      {runtimeKind === 'external_cli' && (
        <ExternalRuntimeFields
          adapter={form.watch('external_adapter')}
          executable={form.watch('external_executable') ?? ''}
          timeoutSeconds={form.watch('external_timeout_seconds')}
          maxTurns={form.watch('external_max_turns')}
          onAdapterChange={(adapter: ExternalRuntimeAdapter) =>
            form.setValue('external_adapter', adapter)
          }
          onExecutableChange={(value) => form.setValue('external_executable', value)}
          onTimeoutSecondsChange={(value) => form.setValue('external_timeout_seconds', value)}
          onMaxTurnsChange={(value) => form.setValue('external_max_turns', value)}
        />
      )}

      {runtimeKind === 'llm_chat' && (
        <>
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
                  {p.name} - {p.kind} - {p.default_model}
                </option>
              ))}
            </select>
          </div>

          <div className="space-y-1.5">
            <button
              type="button"
              className="flex items-center gap-1 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
              onClick={() => setShowAdvanced(!showAdvanced)}
            >
              {showAdvanced ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              Model Parameters
            </button>
            {showAdvanced && (
              <div className="space-y-4 rounded-md border border-border bg-card p-4">
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <Label>Temperature</Label>
                    <span className="text-xs text-muted-foreground">
                      {form.watch('temperature')?.toFixed(1)}
                    </span>
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
                    <span className="text-xs text-muted-foreground">
                      {form.watch('top_p')?.toFixed(2)}
                    </span>
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

          <section className="space-y-2 rounded-md border border-border bg-card p-3">
            <div>
              <h3 className="text-sm font-medium">Built-in tools</h3>
              <p className="text-[11px] text-muted-foreground">
                Select tool permissions to include in the agent context.
              </p>
            </div>
            {builtinTools.isLoading && (
              <p className="text-xs text-muted-foreground">Loading tools...</p>
            )}
            {tools.length > 0 && (
              <ToolSelector
                tools={tools}
                value={currentToolConfig}
                workspaceBackendType={selectedWorkspace?.backend_type ?? 'local'}
                agents={agents.data ?? []}
                currentAgentId={agent.id}
                onChange={setToolConfig}
              />
            )}
          </section>
        </>
      )}

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">Skills</h3>
          <p className="text-[11px] text-muted-foreground">
            Enabled skills are mounted into the prompt through <code>skill_ids</code>.
          </p>
        </div>
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
                      <span className="block max-w-48 truncate opacity-75">
                        {s.description}
                      </span>
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
      <Button type="submit" disabled={update.isPending}>
        {update.isPending ? 'Saving...' : 'Save'}
      </Button>
    </form>
  )
}
