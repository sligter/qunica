import { useEffect, useMemo, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm, type Path, type PathValue } from 'react-hook-form'
import { Link } from 'react-router-dom'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { Switch } from '@/components/ui/switch'
import { useUpdateGroup } from '@/hooks/useGroups'
import { useProviderModels, useProviders } from '@/hooks/useProviders'
import { ApiError } from '@/lib/api-v2/client'
import type { GroupSchedulerConfig } from '@/lib/api-v2/types'
import type { GroupRead, GroupUpdate } from '@/types/api'

const NO_SELECTION = '__none__'
const AUTO_MAX_AGENT_STEPS = 8

const schedulerFormSchema = z
  .object({
    scheduler_enabled: z.boolean(),
    agent_mention_policy: z.enum(['display_only', 'bounded_schedule']),
    max_agent_steps_mode: z.enum(['auto', 'custom']),
    max_agent_steps_custom: z.number().int().min(1, 'Must be at least 1'),
    max_steps_per_agent: z.number().int().min(1, 'Must be at least 1'),
    max_scheduler_hops: z.number().int().min(0, 'Must be 0 or greater'),
    max_moderator_calls: z.number().int().min(0, 'Must be 0 or greater'),
    max_consecutive_failures: z.number().int().min(1, 'Must be at least 1'),
    max_total_failures: z.number().int().min(1, 'Must be at least 1'),
    max_total_tokens: z.number().int().min(1, 'Must be at least 1'),
    turn_timeout_seconds: z
      .number()
      .int()
      .min(1, 'Must be at least 1 second')
      .max(3600, 'Must be 3600 seconds or less'),
    moderator_enabled: z.boolean(),
    moderator_provider_id: z.string().nullable(),
    moderator_model: z.string().nullable(),
  })
  .superRefine((values, context) => {
    if (!values.moderator_enabled) return

    if (!values.moderator_provider_id?.trim()) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ['moderator_provider_id'],
        message: 'Choose an active provider for the moderator',
      })
    }
    if (!values.moderator_model?.trim()) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ['moderator_model'],
        message: 'Choose a model for the moderator',
      })
    }
  })

type SchedulerFormValues = z.infer<typeof schedulerFormSchema>

interface GroupSchedulerSettingsSectionProps {
  group: GroupRead
}

function groupToFormValues(group: GroupSchedulerConfig): SchedulerFormValues {
  return {
    scheduler_enabled: group.scheduler_enabled,
    agent_mention_policy: group.agent_mention_policy,
    max_agent_steps_mode: group.max_agent_steps === null ? 'auto' : 'custom',
    max_agent_steps_custom: group.max_agent_steps ?? AUTO_MAX_AGENT_STEPS,
    max_steps_per_agent: group.max_steps_per_agent,
    max_scheduler_hops: group.max_scheduler_hops,
    max_moderator_calls: group.max_moderator_calls,
    max_consecutive_failures: group.max_consecutive_failures,
    max_total_failures: group.max_total_failures,
    max_total_tokens: group.max_total_tokens,
    turn_timeout_seconds: group.turn_timeout_seconds,
    moderator_enabled: group.moderator_enabled,
    moderator_provider_id: group.moderator_provider_id,
    moderator_model: group.moderator_model,
  }
}

function isTopologyError(error: unknown): boolean {
  return error instanceof ApiError && /topology|hub|leader|ring/i.test(error.message)
}

function errorMessage(error: unknown): string {
  return error instanceof ApiError ? error.message : 'Failed to update scheduler settings'
}

function numericRegistration() {
  return {
    setValueAs: (value: string) => (value === '' ? Number.NaN : Number(value)),
  }
}

function FieldError({ message }: { message: string | undefined }) {
  return message ? (
    <p className="text-xs text-destructive" role="alert">
      {message}
    </p>
  ) : null
}

export function GroupSchedulerSettingsSection({ group }: GroupSchedulerSettingsSectionProps) {
  const update = useUpdateGroup(group.id)
  const providers = useProviders()
  const form = useForm<SchedulerFormValues>({
    resolver: zodResolver(schedulerFormSchema),
    defaultValues: groupToFormValues(group),
  })
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [topologyError, setTopologyError] = useState(false)

  const schedulerEnabled = form.watch('scheduler_enabled')
  const moderatorEnabled = form.watch('moderator_enabled')
  const selectedProviderId = form.watch('moderator_provider_id')
  const selectedModel = form.watch('moderator_model')
  const maxAgentStepsMode = form.watch('max_agent_steps_mode')
  const models = useProviderModels(selectedProviderId ?? undefined)

  const activeProviders = useMemo(
    () => (providers.data ?? []).filter((provider) => provider.status === 'active'),
    [providers.data],
  )
  const selectedProviderIsActive = activeProviders.some(
    (provider) => provider.id === selectedProviderId,
  )
  const providerOptions = useMemo(() => {
    if (!selectedProviderId || selectedProviderIsActive) return activeProviders
    return [
      ...activeProviders,
      {
        id: selectedProviderId,
        name: `Saved provider (${selectedProviderId})`,
        kind: 'openai-compatible' as const,
        base_url: null,
        api_key_masked: '',
        default_model: '',
        context_window_tokens: null,
        context_output_reserve_ratio: null,
        description: null,
        reasoning_passback: false,
        status: 'unavailable',
        created_at: '',
      },
    ]
  }, [activeProviders, selectedProviderId, selectedProviderIsActive])

  const modelOptions = useMemo(() => {
    const available = models.data ?? []
    if (!selectedModel || available.some((model) => model.id === selectedModel)) return available
    return [{ id: selectedModel, name: `Saved model (${selectedModel})` }, ...available]
  }, [models.data, selectedModel])

  // Query invalidation refreshes the group after other settings save. Keep this
  // independent form intact until it is pristine or this form saved successfully.
  useEffect(() => {
    if (!form.formState.isDirty) {
      form.reset(groupToFormValues(group))
      setSubmitError(null)
      setTopologyError(false)
    }
  }, [form, group])

  const schedulerControlsDisabled = !schedulerEnabled || update.isPending
  const moderatorControlsDisabled = schedulerControlsDisabled || !moderatorEnabled

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    setTopologyError(false)

    if (values.moderator_enabled && !selectedProviderIsActive) {
      form.setError('moderator_provider_id', {
        message: 'Choose an active provider for the moderator',
      })
      return
    }

    const payload: GroupUpdate = {
      scheduler_enabled: values.scheduler_enabled,
      agent_mention_policy: values.agent_mention_policy,
      max_agent_steps:
        values.max_agent_steps_mode === 'auto' ? null : values.max_agent_steps_custom,
      max_steps_per_agent: values.max_steps_per_agent,
      max_scheduler_hops: values.max_scheduler_hops,
      max_moderator_calls: values.max_moderator_calls,
      max_consecutive_failures: values.max_consecutive_failures,
      max_total_failures: values.max_total_failures,
      max_total_tokens: values.max_total_tokens,
      turn_timeout_seconds: values.turn_timeout_seconds,
      moderator_enabled: values.moderator_enabled,
      moderator_provider_id: values.moderator_provider_id,
      moderator_model: values.moderator_model,
    }

    try {
      const updated = await update.mutateAsync(payload)
      form.reset(groupToFormValues(updated))
    } catch (error) {
      setSubmitError(errorMessage(error))
      setTopologyError(isTopologyError(error))
    }
  })

  const updateValue = <TField extends Path<SchedulerFormValues>>(
    field: TField,
    value: PathValue<SchedulerFormValues, TField>,
  ) => {
    setSubmitError(null)
    setTopologyError(false)
    form.setValue(field, value, { shouldDirty: true, shouldValidate: true })
  }

  return (
    <SettingsSection
      title="Bounded scheduler"
      description="Configure persistent, budgeted agent collaboration. Changes apply together."
      aside={
        <Button type="submit" size="sm" form="group-scheduler-settings" disabled={!form.formState.isDirty || update.isPending}>
          {update.isPending ? 'Saving...' : 'Save'}
        </Button>
      }
    >
      <form
        id="group-scheduler-settings"
        onSubmit={onSubmit}
        noValidate
        className="divide-y divide-border"
      >
        <SettingsRow
          label="Enable bounded scheduler"
          description="Use the bounded scheduler for future group turns."
        >
          <Switch
            checked={schedulerEnabled}
            onCheckedChange={(value) => updateValue('scheduler_enabled', value)}
            disabled={update.isPending}
            aria-label="Enable bounded scheduler"
          />
        </SettingsRow>

        <SettingsRow
          label="Agent mention policy"
          description="Controls whether agent mentions can schedule bounded follow-ups."
        >
          <Select
            value={form.watch('agent_mention_policy')}
            onValueChange={(value) => {
              if (value === 'display_only' || value === 'bounded_schedule') {
                updateValue('agent_mention_policy', value)
              }
            }}
            disabled={schedulerControlsDisabled}
          >
            <SelectTrigger className="w-52" aria-label="Agent mention policy">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="display_only">Display only</SelectItem>
              <SelectItem value="bounded_schedule">Bounded schedule</SelectItem>
            </SelectContent>
          </Select>
        </SettingsRow>

        <SettingsRow
          label="Maximum agent steps"
          description="Auto derives 3 times the active agents, with a minimum of 8 and maximum of 24."
        >
          <div className="flex items-center gap-2">
            <Select
              value={maxAgentStepsMode}
              onValueChange={(value) => {
                if (value === 'auto' || value === 'custom') {
                  updateValue('max_agent_steps_mode', value)
                }
              }}
              disabled={schedulerControlsDisabled}
            >
              <SelectTrigger className="w-72" aria-label="Maximum agent steps mode">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">Auto (3x agents, min 8, max 24)</SelectItem>
                <SelectItem value="custom">Custom</SelectItem>
              </SelectContent>
            </Select>
            <div className="space-y-1">
              <Input
                aria-label="Custom maximum agent steps"
                type="number"
                min={1}
                step={1}
                className="w-20"
                disabled={schedulerControlsDisabled || maxAgentStepsMode !== 'custom'}
                {...form.register('max_agent_steps_custom', numericRegistration())}
              />
              <FieldError message={form.formState.errors.max_agent_steps_custom?.message} />
            </div>
          </div>
        </SettingsRow>

        <SettingsRow
          label="Steps per agent"
          description="Maximum visible scheduler steps for one agent."
          htmlFor="scheduler-max-steps-per-agent"
        >
          <div className="space-y-1">
            <Input
              id="scheduler-max-steps-per-agent"
              type="number"
              min={1}
              step={1}
              className="w-20"
              disabled={schedulerControlsDisabled}
              {...form.register('max_steps_per_agent', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_steps_per_agent?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Scheduler hops" htmlFor="scheduler-max-hops">
          <div className="space-y-1">
            <Input
              id="scheduler-max-hops"
              type="number"
              min={0}
              step={1}
              className="w-20"
              disabled={schedulerControlsDisabled}
              {...form.register('max_scheduler_hops', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_scheduler_hops?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Moderator calls" htmlFor="scheduler-max-moderator-calls">
          <div className="space-y-1">
            <Input
              id="scheduler-max-moderator-calls"
              type="number"
              min={0}
              step={1}
              className="w-20"
              disabled={schedulerControlsDisabled}
              {...form.register('max_moderator_calls', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_moderator_calls?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Consecutive failures" htmlFor="scheduler-max-consecutive-failures">
          <div className="space-y-1">
            <Input
              id="scheduler-max-consecutive-failures"
              type="number"
              min={1}
              step={1}
              className="w-20"
              disabled={schedulerControlsDisabled}
              {...form.register('max_consecutive_failures', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_consecutive_failures?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Total failures" htmlFor="scheduler-max-total-failures">
          <div className="space-y-1">
            <Input
              id="scheduler-max-total-failures"
              type="number"
              min={1}
              step={1}
              className="w-20"
              disabled={schedulerControlsDisabled}
              {...form.register('max_total_failures', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_total_failures?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Total tokens" htmlFor="scheduler-max-total-tokens">
          <div className="space-y-1">
            <Input
              id="scheduler-max-total-tokens"
              type="number"
              min={1}
              step={1}
              className="w-28"
              disabled={schedulerControlsDisabled}
              {...form.register('max_total_tokens', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_total_tokens?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Turn timeout" description="Seconds, from 1 to 3600." htmlFor="scheduler-turn-timeout">
          <div className="space-y-1">
            <Input
              id="scheduler-turn-timeout"
              type="number"
              min={1}
              max={3600}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('turn_timeout_seconds', numericRegistration())}
            />
            <FieldError message={form.formState.errors.turn_timeout_seconds?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Enable moderator" description="Ask a provider to select among legal candidates.">
          <Switch
            checked={moderatorEnabled}
            onCheckedChange={(value) => updateValue('moderator_enabled', value)}
            disabled={schedulerControlsDisabled}
            aria-label="Enable moderator"
          />
        </SettingsRow>

        <SettingsRow label="Moderator provider">
          <div className="space-y-1">
            <Select
              value={selectedProviderId ?? NO_SELECTION}
              onValueChange={(value) =>
                updateValue('moderator_provider_id', value === NO_SELECTION ? null : value)
              }
              disabled={moderatorControlsDisabled || providers.isLoading}
            >
              <SelectTrigger className="w-64" aria-label="Moderator provider">
                <SelectValue placeholder={providers.isLoading ? 'Loading providers...' : 'Choose provider'} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_SELECTION}>No provider</SelectItem>
                {providerOptions.map((provider) => (
                  <SelectItem
                    key={provider.id}
                    value={provider.id}
                    disabled={provider.status !== 'active'}
                  >
                    {provider.name} - {provider.default_model || 'unavailable'}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <FieldError message={form.formState.errors.moderator_provider_id?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label="Moderator model">
          <div className="space-y-1">
            <Select
              value={selectedModel ?? NO_SELECTION}
              onValueChange={(value) =>
                updateValue('moderator_model', value === NO_SELECTION ? null : value)
              }
              disabled={moderatorControlsDisabled || !selectedProviderId || models.isLoading}
            >
              <SelectTrigger className="w-64" aria-label="Moderator model">
                <SelectValue placeholder={models.isLoading ? 'Loading models...' : 'Choose model'} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_SELECTION}>No model</SelectItem>
                {modelOptions.map((model) => (
                  <SelectItem key={model.id} value={model.id}>
                    {model.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <FieldError message={form.formState.errors.moderator_model?.message} />
          </div>
        </SettingsRow>

        {submitError ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {submitError}{' '}
            {topologyError ? (
              <Link className="underline" to={`/groups/${group.id}/manage?tab=members`}>
                Review group members
              </Link>
            ) : null}
          </p>
        ) : null}
      </form>
    </SettingsSection>
  )
}
