import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { z } from 'zod'

import {
  formatAcpArgs,
  formatAcpEnv,
  parseAcpArgs,
  parseAcpEnv,
} from '@/components/agents/acpRuntimeConfig'
import { ExternalRuntimeFields } from '@/components/agents/ExternalRuntimeFields'
import { SystemPromptMentionTextarea } from '@/components/agents/SystemPromptMentionTextarea'
import { ThinkingLevelControl } from '@/components/agents/ThinkingLevelControl'
import {
  AGENT_TEMPERATURE_STEP,
  DEFAULT_AGENT_SYSTEM_PROMPT,
  DEFAULT_AGENT_TEMPERATURE,
  formatAgentTemperature,
  isAgentTemperatureStep,
  normalizeAgentTemperature,
} from '@/components/agents/defaults'
import { thinkingLevelValues } from '@/components/agents/thinkingLevel'
import { ToolSelector } from '@/components/agents/ToolSelector'
import { createDefaultToolConfig } from '@/components/agents/toolConfig'
import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { useAcpRuntimePresets } from '@/hooks/useAcpRuntimePresets'
import { useAgents } from '@/hooks/useAgents'
import { useBuiltinTools } from '@/hooks/useBuiltinTools'
import { useCreateAgent } from '@/hooks/useCreateAgent'
import { useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type {
  AcpPermissionPolicy,
  AcpRuntimePresetRead,
  AcpRuntimeProfile,
  AgentRuntimeKind,
  AgentToolConfig,
} from '@/types/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  system_prompt: z.string().min(1, 'Required'),
  runtime_kind: z.enum(['llm_chat', 'acp']),
  acp_profile: z.enum(['custom', 'codex', 'claude']),
  acp_command: z.string().optional(),
  acp_args: z.string().optional(),
  acp_env: z.string().optional(),
  acp_timeout_seconds: z.number().int().min(1).max(21600),
  acp_permission_policy: z.enum(['deny', 'auto_allow']),
  acp_model: z.string().optional(),
  acp_mode: z.string().optional(),
  acp_thinking_effort: z.string().optional(),
  llm_provider_id: z.string().optional(),
  workspace_id: z.string().min(1, 'Workspace is required'),
  temperature: z
    .number()
    .min(0)
    .max(2)
    .refine(isAgentTemperatureStep, 'Must use 0.05 increments')
    .optional(),
  top_p: z.number().min(0).max(1).optional(),
  reasoning_effort: z.enum(thinkingLevelValues),
  context_window_tokens: z.preprocess(
    (value) => (value === '' || Number.isNaN(value) ? undefined : value),
    z.number().int().min(1).optional(),
  ),
  context_output_reserve_percent: z.preprocess(
    (value) => (value === '' || Number.isNaN(value) ? undefined : value),
    z.number().min(1).max(90).optional(),
  ),
})

type FormValues = z.infer<typeof schema>

function optionalText(value: string | undefined): string | null {
  const trimmed = value?.trim() ?? ''
  return trimmed ? trimmed : null
}

interface CreateAgentFormProps {
  onCreated?: (newAgentId: string) => void
}

export function CreateAgentForm({ onCreated }: CreateAgentFormProps = {}) {
  const createAgent = useCreateAgent()
  const providers = useProviders()
  const skills = useSkills()
  const workspaces = useWorkspaces()
  const builtinTools = useBuiltinTools()
  const agents = useAgents()
  const acpRuntimePresets = useAcpRuntimePresets()
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [submittedName, setSubmittedName] = useState<string | null>(null)
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>([])
  const [toolConfig, setToolConfig] = useState<AgentToolConfig | null>(null)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const autoAppliedAcpPreset = useRef(false)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      description: '',
      system_prompt: DEFAULT_AGENT_SYSTEM_PROMPT,
      runtime_kind: 'llm_chat',
      acp_profile: 'custom',
      acp_command: '',
      acp_args: '',
      acp_env: '',
      acp_timeout_seconds: 3600,
      acp_permission_policy: 'deny',
      acp_model: '',
      acp_mode: '',
      acp_thinking_effort: '',
      llm_provider_id: '',
      workspace_id: '',
      temperature: DEFAULT_AGENT_TEMPERATURE,
      top_p: 1,
      reasoning_effort: 'default',
      context_window_tokens: undefined,
      context_output_reserve_percent: undefined,
    },
  })

  const runtimeKind = form.watch('runtime_kind')
  const acpPresets = useMemo(
    () => acpRuntimePresets.data?.presets ?? [],
    [acpRuntimePresets.data?.presets],
  )
  const tools = builtinTools.data?.tools ?? []
  const selectedWorkspace = (workspaces.data ?? []).find(
    (workspace) => workspace.id === form.watch('workspace_id'),
  )
  const currentToolConfig = toolConfig ?? createDefaultToolConfig(tools)
  const systemPromptField = form.register('system_prompt')

  const applyAcpPreset = useCallback(
    (preset: AcpRuntimePresetRead) => {
      form.setValue('acp_profile', preset.profile)
      form.setValue('acp_command', preset.command ?? '')
      form.setValue('acp_args', formatAcpArgs(preset.args))
      form.setValue('acp_env', formatAcpEnv(preset.env))
      form.setValue('acp_timeout_seconds', preset.timeout_seconds)
      form.setValue('acp_permission_policy', preset.permission_policy)
      form.setValue('acp_model', preset.default_model ?? '')
      form.setValue('acp_mode', preset.default_mode ?? '')
      form.setValue('acp_thinking_effort', preset.default_thinking_effort ?? '')
    },
    [form],
  )

  useEffect(() => {
    if (runtimeKind !== 'acp' || autoAppliedAcpPreset.current) {
      return
    }
    const preset = acpPresets.find((item) => item.installed)
    if (!preset) {
      return
    }
    applyAcpPreset(preset)
    autoAppliedAcpPreset.current = true
  }, [acpPresets, applyAcpPreset, runtimeKind])

  const toggleSkill = (id: string) => {
    setSelectedSkillIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    )
  }

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    setSubmittedName(null)
    try {
      const llm_config: Record<string, unknown> = {}
      if (values.temperature !== undefined) llm_config.temperature = values.temperature
      if (values.top_p !== undefined && values.top_p !== 1) llm_config.top_p = values.top_p
      if (values.reasoning_effort !== 'default') {
        llm_config.reasoning_effort = values.reasoning_effort
      }
      if (values.context_window_tokens !== undefined) {
        llm_config.context_window_tokens = values.context_window_tokens
      }
      if (values.context_output_reserve_percent !== undefined) {
        llm_config.context_output_reserve_ratio =
          values.context_output_reserve_percent / 100
      }

      const created = await createAgent.mutateAsync({
        name: values.name,
        description: values.description,
        system_prompt: values.system_prompt,
        runtime_kind: values.runtime_kind,
        acp_runtime:
          values.runtime_kind === 'acp'
            ? {
                profile: values.acp_profile,
                command: values.acp_command?.trim() ?? '',
                args: parseAcpArgs(values.acp_args ?? ''),
                env: parseAcpEnv(values.acp_env ?? ''),
                timeout_seconds: values.acp_timeout_seconds,
                permission_policy: values.acp_permission_policy,
                model: optionalText(values.acp_model),
                mode: optionalText(values.acp_mode),
                thinking_effort: optionalText(values.acp_thinking_effort),
                config_options: null,
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
        <SystemPromptMentionTextarea
          id="agent-system-prompt"
          rows={5}
          placeholder="You are a concise assistant. Type @ to insert Agent or Team context."
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
            ['acp', 'ACP', 'Agent Client Protocol process'],
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
      </section>

      {runtimeKind === 'acp' && (
        <ExternalRuntimeFields
          presets={acpPresets}
          selectedProfile={form.watch('acp_profile')}
          command={form.watch('acp_command') ?? ''}
          argsText={form.watch('acp_args') ?? ''}
          envText={form.watch('acp_env') ?? ''}
          timeoutSeconds={form.watch('acp_timeout_seconds')}
          permissionPolicy={form.watch('acp_permission_policy')}
          model={form.watch('acp_model') ?? ''}
          mode={form.watch('acp_mode') ?? ''}
          thinkingEffort={form.watch('acp_thinking_effort') ?? ''}
          onProfileChange={(value: AcpRuntimeProfile) => form.setValue('acp_profile', value)}
          onPresetSelect={applyAcpPreset}
          onCommandChange={(value) => form.setValue('acp_command', value)}
          onArgsTextChange={(value) => form.setValue('acp_args', value)}
          onEnvTextChange={(value) => form.setValue('acp_env', value)}
          onTimeoutSecondsChange={(value) => form.setValue('acp_timeout_seconds', value)}
          onPermissionPolicyChange={(value: AcpPermissionPolicy) =>
            form.setValue('acp_permission_policy', value)
          }
          onModelChange={(value) => form.setValue('acp_model', value)}
          onModeChange={(value) => form.setValue('acp_mode', value)}
          onThinkingEffortChange={(value) => form.setValue('acp_thinking_effort', value)}
        />
      )}

      {runtimeKind === 'llm_chat' && (
        <>
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
                  {p.name} - {p.kind} - {p.default_model}
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
                      {formatAgentTemperature(form.watch('temperature'))}
                    </span>
                  </div>
                  <Slider
                    min={0}
                    max={2}
                    step={AGENT_TEMPERATURE_STEP}
                    value={[form.watch('temperature') ?? DEFAULT_AGENT_TEMPERATURE]}
                    onValueChange={([v]) =>
                      form.setValue('temperature', normalizeAgentTemperature(v))
                    }
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
                <ThinkingLevelControl
                  value={form.watch('reasoning_effort')}
                  onChange={(value) => form.setValue('reasoning_effort', value)}
                />
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="space-y-1.5">
                    <Label htmlFor="agent-context-window">Context window override</Label>
                    <Input
                      id="agent-context-window"
                      type="number"
                      min={1}
                      placeholder="Inherit provider"
                      {...form.register('context_window_tokens', { valueAsNumber: true })}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="agent-output-reserve">Output reserve % override</Label>
                    <Input
                      id="agent-output-reserve"
                      type="number"
                      min={1}
                      max={90}
                      placeholder="Inherit provider"
                      {...form.register('context_output_reserve_percent', {
                        valueAsNumber: true,
                      })}
                    />
                  </div>
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
        {skills.isLoading && <p className="text-xs text-muted-foreground">Loading...</p>}
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
      {submittedName && (
        <p className="text-sm text-green-700">Created agent: {submittedName}</p>
      )}
      <Button type="submit" disabled={createAgent.isPending}>
        {createAgent.isPending ? 'Creating...' : 'Create agent'}
      </Button>
    </form>
  )
}
