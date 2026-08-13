import { useEffect, useMemo, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm, type Path, type PathValue } from 'react-hook-form'
import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
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
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'
import type { GroupSchedulerConfig } from '@/lib/api-v2/types'
import type { GroupRead, GroupUpdate } from '@/types/api'

const NO_SELECTION = '__none__'
const UNKNOWN_POLICY_PREFIX = '__unknown_policy__:'
const AUTO_MAX_AGENT_STEPS = 8
type AgentMentionPolicy = GroupSchedulerConfig['agent_mention_policy']

const mentionPolicies: AgentMentionPolicy[] = ['display_only', 'bounded_schedule']
const mentionPolicyKeys = {
  display_only: 'scheduler.displayOnly',
  bounded_schedule: 'scheduler.boundedSchedule',
} as const satisfies Record<AgentMentionPolicy, string>

function isAgentMentionPolicy(value: string): value is AgentMentionPolicy {
  return mentionPolicies.some((policy) => policy === value)
}

const schedulerFormSchema = z
  .object({
    agent_mention_policy: z.string(),
    max_agent_steps_mode: z.enum(['auto', 'custom']),
    max_agent_steps_custom: z.number().int().min(1, 'minOne'),
    max_steps_per_agent: z.number().int().min(1, 'minOne'),
    max_scheduler_hops: z.number().int().min(0, 'minZero'),
    max_moderator_calls: z.number().int().min(0, 'minZero'),
    max_consecutive_failures: z.number().int().min(1, 'minOne'),
    max_total_failures: z.number().int().min(1, 'minOne'),
    max_total_tokens: z.number().int().min(1, 'minOne'),
    turn_timeout_seconds: z
      .number()
      .int()
      .min(1, 'minOneSecond')
      .max(3600, 'maxSeconds'),
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
        message: 'provider',
      })
    }
    if (!values.moderator_model?.trim()) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ['moderator_model'],
        message: 'model',
      })
    }
  })

type SchedulerFormValues = z.infer<typeof schedulerFormSchema>

interface GroupSchedulerSettingsSectionProps {
  group: GroupRead
}

function groupToFormValues(group: GroupSchedulerConfig): SchedulerFormValues {
  return {
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

function numericRegistration() {
  return {
    setValueAs: (value: string) => (value === '' ? Number.NaN : Number(value)),
  }
}

function FieldError({ message }: { message: string | undefined }) {
  const { t } = useTranslation('groups')
  const knownMessages = [
    'minOne',
    'minZero',
    'minOneSecond',
    'maxSeconds',
    'provider',
    'model',
  ]
  return message ? (
    <p className="text-xs text-destructive" role="alert">
      {knownMessages.includes(message)
        ? t(`scheduler.validation.${message}`)
        : t('scheduler.validation.detail', { message })}
    </p>
  ) : null
}

export function GroupSchedulerSettingsSection({ group }: GroupSchedulerSettingsSectionProps) {
  const { t, i18n } = useTranslation(['groups', 'common'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const update = useUpdateGroup(group.id)
  const providers = useProviders()
  const form = useForm<SchedulerFormValues>({
    resolver: zodResolver(schedulerFormSchema),
    defaultValues: groupToFormValues(group),
  })
  const [submitError, setSubmitError] = useState<string | null | undefined>(undefined)
  const [topologyError, setTopologyError] = useState(false)

  const moderatorEnabled = form.watch('moderator_enabled')
  const selectedProviderId = form.watch('moderator_provider_id')
  const selectedModel = form.watch('moderator_model')
  const maxAgentStepsMode = form.watch('max_agent_steps_mode')
  const mentionPolicy = form.watch('agent_mention_policy') as string
  const mentionPolicySelectValue = isAgentMentionPolicy(mentionPolicy)
    ? mentionPolicy
    : `${UNKNOWN_POLICY_PREFIX}${mentionPolicy}`
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
        name: t('scheduler.savedProvider', { id: selectedProviderId }),
        kind: 'openai-compatible' as const,
        base_url: null,
        api_key_masked: '',
        default_model: '',
        context_window_tokens: null,
        context_output_reserve_ratio: null,
        description: null,
        reasoning_passback: false,
        models: [],
        status: 'unavailable',
        created_at: '',
      },
    ]
  }, [activeProviders, selectedProviderId, selectedProviderIsActive, t])

  const modelOptions = useMemo(() => {
    const provider = providerOptions.find((item) => item.id === selectedProviderId)
    const configured = provider?.models ?? (
      provider?.default_model
        ? [{
            id: provider.default_model,
            context_window_tokens: provider.context_window_tokens,
            context_output_reserve_ratio: provider.context_output_reserve_ratio,
          }]
        : []
    )
    const available = configured.map((configuredModel) =>
      models.data?.find((model) => model.id === configuredModel.id) ?? {
        id: configuredModel.id,
        name: configuredModel.id,
      },
    )
    if (!selectedModel || available.some((model) => model.id === selectedModel)) return available
    return [{ id: selectedModel, name: t('scheduler.savedModel', { id: selectedModel }) }, ...available]
  }, [models.data, providerOptions, selectedModel, selectedProviderId, t])

  // Query invalidation refreshes the group after other settings save. Keep this
  // independent form intact until it is pristine or this form saved successfully.
  useEffect(() => {
    if (!form.formState.isDirty) {
      form.reset(groupToFormValues(group))
      setSubmitError(undefined)
      setTopologyError(false)
    }
  }, [form, group])

  const schedulerControlsDisabled = update.isPending
  const moderatorControlsDisabled = update.isPending || !moderatorEnabled

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(undefined)
    setTopologyError(false)

    if (values.moderator_enabled && !selectedProviderIsActive) {
      form.setError('moderator_provider_id', {
        message: 'provider',
      })
      return
    }

    const payload: GroupUpdate = {
      agent_mention_policy:
        values.agent_mention_policy as GroupUpdate['agent_mention_policy'],
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
      setSubmitError(error instanceof ApiError ? error.message : null)
      setTopologyError(isTopologyError(error))
    }
  })

  const updateValue = <TField extends Path<SchedulerFormValues>>(
    field: TField,
    value: PathValue<SchedulerFormValues, TField>,
  ) => {
    setSubmitError(undefined)
    setTopologyError(false)
    form.setValue(field, value, { shouldDirty: true, shouldValidate: true })
  }

  const onProviderChange = (value: string) => {
    const providerId = value === NO_SELECTION ? null : value
    if (providerId === selectedProviderId) return

    updateValue('moderator_provider_id', providerId)
    updateValue('moderator_model', null)
  }

  return (
    <SettingsSection
      title={t('scheduler.title')}
      description={t('scheduler.description')}
      aside={
        <Button type="submit" size="sm" form="group-scheduler-settings" disabled={!form.formState.isDirty || update.isPending}>
          {update.isPending ? t('common:actions.saving') : t('common:actions.save')}
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
          label={t('scheduler.mentionPolicy')}
          description={t('scheduler.mentionPolicyDescription')}
        >
          <Select
            value={mentionPolicySelectValue}
            onValueChange={(value) => {
              if (isAgentMentionPolicy(value)) {
                updateValue('agent_mention_policy', value)
              }
            }}
            disabled={schedulerControlsDisabled}
          >
            <SelectTrigger className="w-52" aria-label={t('scheduler.mentionPolicy')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {!isAgentMentionPolicy(mentionPolicy) ? (
                <SelectItem value={mentionPolicySelectValue}>
                  {t('scheduler.unknownMentionPolicy', { value: mentionPolicy })}
                </SelectItem>
              ) : null}
              {mentionPolicies.map((policy) => (
                <SelectItem key={policy} value={policy}>
                  {t(mentionPolicyKeys[policy])}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>

        <SettingsRow
          label={t('scheduler.maximumSteps')}
          description={t('scheduler.maximumStepsDescription', {
            factor: formatNumber(3, language),
            minimum: formatNumber(8, language),
            maximum: formatNumber(24, language),
          })}
          stacked
        >
          <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_6rem]">
            <Select
              value={maxAgentStepsMode}
              onValueChange={(value) => {
                if (value === 'auto' || value === 'custom') {
                  updateValue('max_agent_steps_mode', value)
                }
              }}
              disabled={schedulerControlsDisabled}
            >
              <SelectTrigger className="w-full" aria-label={t('scheduler.maximumStepsMode')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">{t('scheduler.autoSteps', {
                  factor: formatNumber(3, language),
                  minimum: formatNumber(8, language),
                  maximum: formatNumber(24, language),
                })}</SelectItem>
                <SelectItem value="custom">{t('scheduler.custom')}</SelectItem>
              </SelectContent>
            </Select>
            <div className="space-y-1">
              <Input
                aria-label={t('scheduler.customMaximumSteps')}
                type="number"
                min={1}
                step={1}
                className="w-full"
                disabled={schedulerControlsDisabled || maxAgentStepsMode !== 'custom'}
                {...form.register('max_agent_steps_custom', numericRegistration())}
              />
              <FieldError message={form.formState.errors.max_agent_steps_custom?.message} />
            </div>
          </div>
        </SettingsRow>

        <SettingsRow
          label={t('scheduler.stepsPerAgent')}
          description={t('scheduler.stepsPerAgentDescription')}
          htmlFor="scheduler-max-steps-per-agent"
        >
          <div className="space-y-1">
            <Input
              id="scheduler-max-steps-per-agent"
              type="number"
              min={1}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('max_steps_per_agent', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_steps_per_agent?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.hops')} htmlFor="scheduler-max-hops">
          <div className="space-y-1">
            <Input
              id="scheduler-max-hops"
              type="number"
              min={0}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('max_scheduler_hops', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_scheduler_hops?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.moderatorCalls')} htmlFor="scheduler-max-moderator-calls">
          <div className="space-y-1">
            <Input
              id="scheduler-max-moderator-calls"
              type="number"
              min={0}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('max_moderator_calls', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_moderator_calls?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.consecutiveFailures')} htmlFor="scheduler-max-consecutive-failures">
          <div className="space-y-1">
            <Input
              id="scheduler-max-consecutive-failures"
              type="number"
              min={1}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('max_consecutive_failures', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_consecutive_failures?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.totalFailures')} htmlFor="scheduler-max-total-failures">
          <div className="space-y-1">
            <Input
              id="scheduler-max-total-failures"
              type="number"
              min={1}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('max_total_failures', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_total_failures?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.totalTokens')} htmlFor="scheduler-max-total-tokens">
          <div className="space-y-1">
            <Input
              id="scheduler-max-total-tokens"
              type="number"
              min={1}
              step={1}
              className="w-24"
              disabled={schedulerControlsDisabled}
              {...form.register('max_total_tokens', numericRegistration())}
            />
            <FieldError message={form.formState.errors.max_total_tokens?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.timeout')} description={t('scheduler.timeoutDescription', { minimum: formatNumber(1, language), maximum: formatNumber(3600, language) })} htmlFor="scheduler-turn-timeout">
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

        <SettingsRow label={t('scheduler.enableModerator')} description={t('scheduler.enableModeratorDescription')}>
          <Switch
            checked={moderatorEnabled}
            onCheckedChange={(value) => updateValue('moderator_enabled', value)}
            disabled={schedulerControlsDisabled}
            aria-label={t('scheduler.enableModerator')}
          />
        </SettingsRow>

        <SettingsRow label={t('scheduler.moderatorProvider')} stacked>
          <div className="space-y-1">
            <Select
              value={selectedProviderId ?? NO_SELECTION}
              onValueChange={onProviderChange}
              disabled={moderatorControlsDisabled || providers.isLoading}
            >
              <SelectTrigger className="w-full" aria-label={t('scheduler.moderatorProvider')}>
                <SelectValue placeholder={providers.isLoading ? t('scheduler.loadingProviders') : t('scheduler.chooseProvider')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_SELECTION}>{t('scheduler.noProvider')}</SelectItem>
                {providerOptions.map((provider) => (
                  <SelectItem
                    key={provider.id}
                    value={provider.id}
                    disabled={provider.status !== 'active'}
                  >
                    {provider.name} - {provider.default_model || t('scheduler.unavailable')}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <FieldError message={form.formState.errors.moderator_provider_id?.message} />
          </div>
        </SettingsRow>

        <SettingsRow label={t('scheduler.moderatorModel')} stacked>
          <div className="space-y-1">
            <Select
              value={selectedModel ?? NO_SELECTION}
              onValueChange={(value) =>
                updateValue('moderator_model', value === NO_SELECTION ? null : value)
              }
              disabled={moderatorControlsDisabled || !selectedProviderId || models.isLoading}
            >
              <SelectTrigger className="w-full" aria-label={t('scheduler.moderatorModel')}>
                <SelectValue placeholder={models.isLoading ? t('scheduler.loadingModels') : t('scheduler.chooseModel')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_SELECTION}>{t('scheduler.noModel')}</SelectItem>
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

        {submitError !== undefined ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {submitError
              ? t('errors.updateDetail', { message: submitError })
              : t('scheduler.errors.update')}{' '}
            {topologyError ? (
              <Link className="underline" to={`/groups/${group.id}/manage?tab=members`}>
                {t('scheduler.reviewMembers')}
              </Link>
            ) : null}
          </p>
        ) : null}
      </form>
    </SettingsSection>
  )
}
