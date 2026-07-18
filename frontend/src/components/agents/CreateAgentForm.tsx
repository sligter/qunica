import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { z } from 'zod'

import {
  formatAcpArgs,
  formatAcpEnv,
  parseAcpArgs,
  parseAcpEnv,
} from '@/components/agents/acpRuntimeConfig'
import { ExternalRuntimeFields } from '@/components/agents/ExternalRuntimeFields'
import { RuntimeCapabilityField } from '@/components/agents/RuntimeCapabilityField'
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
import { useCommittedAcpRuntimeCapabilities } from '@/components/agents/useCommittedAcpRuntimeCapabilities'
import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { useAcpRuntimePresets } from '@/hooks/useAcpRuntimePresets'
import { useAgents } from '@/hooks/useAgents'
import { useBuiltinTools } from '@/hooks/useBuiltinTools'
import { useCreateAgent } from '@/hooks/useCreateAgent'
import { useProviderModels, useProviders } from '@/hooks/useProviders'
import { useSkills } from '@/hooks/useSkills'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'
import { cn } from '@/lib/utils'
import type {
  AcpPermissionPolicy,
  AcpRuntimePresetRead,
  AcpRuntimeProfile,
  AgentRuntimeKind,
  AgentToolConfig,
} from '@/types/api'

function createSchema(nameRequired: string, promptRequired: string, workspaceRequired: string, increment: string) {
 return z.object({
  name: z.string().min(1, nameRequired).max(100),
  description: z.string().optional(),
  system_prompt: z.string().min(1, promptRequired),
  runtime_kind: z.enum(['llm_chat', 'acp']),
  acp_profile: z.enum(['custom', 'codex', 'claude', 'pi', 'opencode']),
  acp_command: z.string().optional(),
  acp_args: z.string().optional(),
  acp_env: z.string().optional(),
  acp_timeout_seconds: z.number().int().min(1).max(21600),
  acp_permission_policy: z.enum(['deny', 'auto_allow']),
  acp_model: z.string().optional(),
  acp_mode: z.string().optional(),
  acp_thinking_effort: z.string().optional(),
  llm_provider_id: z.string().optional(),
  model: z.string().optional(),
  workspace_id: z.string().min(1, workspaceRequired),
  temperature: z
    .number()
    .min(0)
    .max(2)
    .refine(isAgentTemperatureStep, increment)
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
}

type FormValues = z.infer<ReturnType<typeof createSchema>>

function optionalText(value: string | undefined): string | null {
  const trimmed = value?.trim() ?? ''
  return trimmed ? trimmed : null
}

interface CreateAgentFormProps {
  onCreated?: (newAgentId: string) => void
}

export function CreateAgentForm({ onCreated }: CreateAgentFormProps = {}) {
  const { t, i18n } = useTranslation(['agents', 'common'])
  const createAgent = useCreateAgent()
  const providers = useProviders()
  const skills = useSkills()
  const workspaces = useWorkspaces()
  const builtinTools = useBuiltinTools()
  const agents = useAgents()
  const acpRuntimePresets = useAcpRuntimePresets()
  const [submitError, setSubmitError] = useState<LocalizedError | null>(null)
  const [submittedName, setSubmittedName] = useState<string | null>(null)
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>([])
  const [toolConfig, setToolConfig] = useState<AgentToolConfig | null>(null)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const autoAppliedAcpPreset = useRef(false)
  const validationLanguage = useRef(i18n.resolvedLanguage)
  const schema = useMemo(
    () => createSchema(
      t('agents:validation.nameRequired'),
      t('agents:validation.systemPromptRequired'),
      t('agents:validation.workspaceRequired'),
      t('agents:validation.temperatureIncrement'),
    ),
    [t],
  )

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
      model: '',
      workspace_id: '',
      temperature: DEFAULT_AGENT_TEMPERATURE,
      top_p: 1,
      reasoning_effort: 'default',
      context_window_tokens: undefined,
      context_output_reserve_percent: undefined,
    },
  })

  useEffect(() => {
    if (validationLanguage.current === i18n.resolvedLanguage) return
    validationLanguage.current = i18n.resolvedLanguage
    if (Object.keys(form.formState.errors).length > 0) {
      void form.trigger()
    }
  }, [form, i18n.resolvedLanguage])

  const runtimeKind = form.watch('runtime_kind')
  const acpPresets = useMemo(
    () => acpRuntimePresets.data?.presets ?? [],
    [acpRuntimePresets.data?.presets],
  )
  const selectedProviderId = form.watch('llm_provider_id') || undefined
  const providerModels = useProviderModels(
    runtimeKind === 'llm_chat' ? selectedProviderId : undefined,
  )
  const acpCapabilities = useCommittedAcpRuntimeCapabilities(
    {
      profile: form.watch('acp_profile'),
      command: form.watch('acp_command') ?? '',
      argsText: form.watch('acp_args') ?? '',
      envText: form.watch('acp_env') ?? '',
      permissionPolicy: form.watch('acp_permission_policy'),
      model: form.watch('acp_model') ?? '',
    },
    runtimeKind === 'acp',
  )
  const tools = builtinTools.data?.tools ?? []
  const selectedWorkspace = (workspaces.data ?? []).find(
    (workspace) => workspace.id === form.watch('workspace_id'),
  )
  const currentToolConfig = toolConfig ?? createDefaultToolConfig(tools)
  const systemPromptField = form.register('system_prompt')

  const applyAcpPreset = useCallback(
    (preset: AcpRuntimePresetRead) => {
      const command = preset.command ?? ''
      const argsText = formatAcpArgs(preset.args)
      const envText = formatAcpEnv(preset.env)
      const model = preset.default_model ?? ''
      form.setValue('acp_profile', preset.profile)
      form.setValue('acp_command', command)
      form.setValue('acp_args', argsText)
      form.setValue('acp_env', envText)
      form.setValue('acp_timeout_seconds', preset.timeout_seconds)
      form.setValue('acp_permission_policy', preset.permission_policy)
      form.setValue('acp_model', model)
      form.setValue('acp_mode', preset.default_mode ?? '')
      form.setValue('acp_thinking_effort', preset.default_thinking_effort ?? '')
      acpCapabilities.commit({
        profile: preset.profile,
        command,
        argsText,
        envText,
        permissionPolicy: preset.permission_policy,
        model,
      })
    },
    [acpCapabilities, form],
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
      const model = values.model?.trim()
      if (model) llm_config.model = model
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
      setSubmitError(err instanceof ApiError ? messageError(err.message) : translatedError('agents:errors.network'))
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="agent-name">{t('agents:fields.name')}</Label>
        <Input id="agent-name" placeholder={t('agents:form.namePlaceholder')} {...form.register('name')} />
        {form.formState.errors.name && (
          <p className="text-xs text-destructive">{form.formState.errors.name.message}</p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="agent-description">{t('agents:fields.descriptionOptional')}</Label>
        <Input
          id="agent-description"
          placeholder={t('agents:form.descriptionPlaceholder')}
          {...form.register('description')}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="agent-system-prompt">{t('agents:fields.systemPrompt')}</Label>
        <SystemPromptMentionTextarea
          id="agent-system-prompt"
          rows={5}
          placeholder={t('agents:form.systemPromptPlaceholder')}
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
          <p className="text-xs text-destructive">
            {form.formState.errors.system_prompt.message}
          </p>
        )}
      </div>

      <section className="space-y-2 rounded-md border border-border bg-card p-3">
        <div>
          <h3 className="text-sm font-medium">{t('agents:fields.runtime')}</h3>
          <p className="text-[11px] text-muted-foreground">
            {t('agents:form.runtimeDescription')}
          </p>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          {([
            ['llm_chat', t('agents:runtime.chatLabel'), t('agents:runtime.chatHint')],
            ['acp', t('agents:runtime.acpLabel'), t('agents:runtime.acpHint')],
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
          <h3 className="text-sm font-medium">{t('agents:fields.workspace')}</h3>
          <p className="text-[11px] text-muted-foreground">
            {t('agents:form.workspaceDescription')}
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
          modelOptions={acpCapabilities.data?.models}
          modeOptions={acpCapabilities.data?.modes}
          thinkingEffortOptions={acpCapabilities.data?.thinking_efforts}
          capabilitiesLoading={acpCapabilities.isFetching}
          capabilitiesStale={acpCapabilities.capabilitiesStale}
          capabilitiesWarning={
            acpCapabilities.data?.warning ??
            (acpCapabilities.isError
              ? t('agents:errors.runtimeCapabilities')
              : null)
          }
          onProfileChange={(value: AcpRuntimeProfile) => {
            form.setValue('acp_profile', value)
            acpCapabilities.commitProfile(value)
          }}
          onPresetSelect={applyAcpPreset}
          onCommandChange={(value) => {
            form.setValue('acp_command', value)
            acpCapabilities.markStale()
          }}
          onArgsTextChange={(value) => {
            form.setValue('acp_args', value)
            acpCapabilities.markStale()
          }}
          onEnvTextChange={(value) => {
            form.setValue('acp_env', value)
            acpCapabilities.markStale()
          }}
          onTimeoutSecondsChange={(value) => form.setValue('acp_timeout_seconds', value)}
          onPermissionPolicyChange={(value: AcpPermissionPolicy) =>
            form.setValue('acp_permission_policy', value)
          }
          onModelChange={(value) => form.setValue('acp_model', value)}
          onModelCommit={acpCapabilities.commitModel}
          onModeChange={(value) => form.setValue('acp_mode', value)}
          onThinkingEffortChange={(value) => form.setValue('acp_thinking_effort', value)}
          onRefreshCapabilities={acpCapabilities.refresh}
        />
      )}

      {runtimeKind === 'llm_chat' && (
        <>
          <div className="space-y-1.5">
            <Label htmlFor="agent-provider">{t('agents:fields.provider')}</Label>
            <select
              id="agent-provider"
              className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              {...form.register('llm_provider_id')}
            >
              <option value="">{t('agents:states.defaultProvider')}</option>
              {(providers.data ?? []).map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} - {p.kind} - {p.default_model}
                </option>
              ))}
            </select>
            {providers.data && providers.data.length === 0 && (
              <p className="text-[11px] text-muted-foreground">
                {t('agents:form.noProviders')}
              </p>
            )}
          </div>

          <RuntimeCapabilityField
            id="agent-model"
            label={t('agents:fields.model')}
            value={form.watch('model') ?? ''}
            options={(providerModels.data ?? []).map((model) => ({
              value: model.id,
              label: model.name,
            }))}
            placeholder={t('agents:states.providerDefault')}
            onChange={(value) => form.setValue('model', value)}
            isLoading={providerModels.isFetching}
            warning={
              providerModels.isError
                ? t('agents:errors.providerModels')
                : null
            }
          />

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
              {t('agents:fields.modelParameters')}
            </button>
            {showAdvanced && (
              <div className="space-y-4 rounded-md border border-border bg-card p-4">
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <Label>{t('agents:fields.temperature')}</Label>
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
                    <Label>{t('agents:fields.topP')}</Label>
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
                    <Label htmlFor="agent-context-window">{t('agents:fields.contextWindowOverride')}</Label>
                    <Input
                      id="agent-context-window"
                      type="number"
                      min={1}
                      placeholder={t('agents:states.inheritProvider')}
                      {...form.register('context_window_tokens', { valueAsNumber: true })}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="agent-output-reserve">{t('agents:fields.outputReserveOverride')}</Label>
                    <Input
                      id="agent-output-reserve"
                      type="number"
                      min={1}
                      max={90}
                      placeholder={t('agents:states.inheritProvider')}
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
              <h3 className="text-sm font-medium">{t('agents:fields.builtInTools')}</h3>
              <p className="text-[11px] text-muted-foreground">
                {t('agents:form.toolsDescription')}
              </p>
            </div>
            {builtinTools.isLoading && (
              <p className="text-xs text-muted-foreground">{t('agents:states.loadingTools')}</p>
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
          <h3 className="text-sm font-medium">{t('agents:fields.skills')}</h3>
          <p className="text-[11px] text-muted-foreground">
            {t('agents:form.skillsDescription')}
          </p>
        </div>
        {skills.isLoading && <p className="text-xs text-muted-foreground">{t('agents:states.loadingSkills')}</p>}
        {skills.data && skills.data.length === 0 && (
          <p className="text-[11px] text-muted-foreground">
            {t('agents:form.noSkillsCreate')}
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

      {localizedErrorText(submitError, t) && (
        <p className="text-sm text-destructive" role="alert">
          {localizedErrorText(submitError, t)}
        </p>
      )}
      {submittedName && (
        <p className="text-sm text-success">{t('agents:form.created', { name: submittedName })}</p>
      )}
      <Button type="submit" disabled={createAgent.isPending}>
        {createAgent.isPending ? t('agents:actions.creating') : t('agents:actions.create')}
      </Button>
    </form>
  )
}
