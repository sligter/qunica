import { useMemo, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { useTranslation } from 'react-i18next'

import { ReasoningPassbackControl } from '@/components/providers/ReasoningPassbackControl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useUpdateProvider } from '@/hooks/useProviders'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type { LLMProviderRead, ProviderKind } from '@/types/api'

function createSchema(required: string) { return z.object({
  name: z.string().min(1, required).max(100),
  kind: z.enum(['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']),
  base_url: z.string().optional(),
  api_key: z.string().optional(),
  default_model: z.string().min(1, required),
  context_window_tokens: z.preprocess(
    (value) => (value === '' || Number.isNaN(value) ? undefined : value),
    z.number().int().min(1).optional(),
  ),
  context_output_reserve_percent: z.number().min(1).max(90),
  description: z.string().optional(),
  reasoning_passback: z.boolean(),
}) }

type FormValues = z.infer<ReturnType<typeof createSchema>>

interface EditProviderFormProps {
  provider: LLMProviderRead
  onSaved?: (providerId: string) => void
}

const KIND_OPTIONS: ProviderKind[] = ['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']
const KIND_KEYS: Record<ProviderKind, 'openai' | 'anthropic' | 'anthropicCompatible' | 'gemini'> = { 'openai-compatible': 'openai', anthropic: 'anthropic', 'anthropic-compatible': 'anthropicCompatible', gemini: 'gemini' }

function baseUrlPlaceholder(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') {
    return 'https://api.anthropic.com'
  }
  if (kind === 'gemini') return 'https://generativelanguage.googleapis.com/v1beta'
  return 'https://api.openai.com/v1'
}

export function EditProviderForm({ provider, onSaved }: EditProviderFormProps) {
  const { t } = useTranslation('providers')
  const update = useUpdateProvider(provider.id)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const schema = useMemo(() => createSchema(t('validation.required')), [t])

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: provider.name,
      kind: provider.kind,
      base_url: provider.base_url ?? '',
      api_key: '',
      default_model: provider.default_model,
      context_window_tokens: provider.context_window_tokens ?? undefined,
      context_output_reserve_percent:
        provider.context_output_reserve_ratio !== null
          ? Math.round(provider.context_output_reserve_ratio * 100)
          : 30,
      description: provider.description ?? '',
      reasoning_passback: provider.reasoning_passback,
    },
  })

  const kind = form.watch('kind')

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      const updated = await update.mutateAsync({
        name: values.name,
        kind: values.kind,
        base_url: values.base_url || null,
        api_key: values.api_key ? values.api_key : undefined,
        default_model: values.default_model,
        context_window_tokens: values.context_window_tokens ?? null,
        context_output_reserve_ratio: values.context_output_reserve_percent / 100,
        description: values.description || null,
        reasoning_passback: values.reasoning_passback,
      })
      onSaved?.(updated.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : t('errors.network'))
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor={`provider-name-${provider.id}`}>{t('fields.name')}</Label>
        <Input id={`provider-name-${provider.id}`} {...form.register('name')} />
        {form.formState.errors.name && (
          <p className="text-xs text-destructive">{form.formState.errors.name.message}</p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label>{t('fields.kind')}</Label>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {KIND_OPTIONS.map((opt) => {
            const checked = kind === opt
            const key = KIND_KEYS[opt]
            return (
              <button
                type="button"
                key={opt}
                onClick={() => form.setValue('kind', opt, { shouldDirty: true })}
                className={cn(
                  'flex min-h-20 flex-col items-start gap-1 rounded-md border px-3 py-2 text-left transition-colors',
                  checked ? 'border-primary bg-primary/10' : 'border-border hover:bg-card-hover',
                )}
              >
                <span className="text-sm font-medium">{t(`kinds.${key}.label`)}</span>
                <span className="text-[11px] leading-snug text-muted-foreground">
                  {t(`kinds.${key}.hint`)}
                </span>
              </button>
            )
          })}
        </div>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor={`provider-base-url-${provider.id}`}>{t('fields.baseUrlOptional')}</Label>
        <Input
          id={`provider-base-url-${provider.id}`}
          placeholder={baseUrlPlaceholder(kind)}
          {...form.register('base_url')}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor={`provider-api-key-${provider.id}`}>{t('fields.apiKey')}</Label>
        <Input
          id={`provider-api-key-${provider.id}`}
          type="password"
          placeholder={t('form.keyMaskedPlaceholder', { masked: provider.api_key_masked })}
          {...form.register('api_key')}
        />
        <p className="text-[11px] text-muted-foreground">
          {t('form.keyMaskedHint')}
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor={`provider-model-${provider.id}`}>{t('fields.defaultModel')}</Label>
        <Input
          id={`provider-model-${provider.id}`}
          placeholder={t(`kinds.${KIND_KEYS[kind]}.modelPlaceholder`)}
          {...form.register('default_model')}
        />
        {form.formState.errors.default_model && (
          <p className="text-xs text-destructive">
            {form.formState.errors.default_model.message}
          </p>
        )}
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor={`provider-context-window-${provider.id}`}>
            {t('fields.contextWindowTokens')}
          </Label>
          <Input
            id={`provider-context-window-${provider.id}`}
            type="number"
            min={1}
            placeholder={t('form.autoFromModel')}
            {...form.register('context_window_tokens', { valueAsNumber: true })}
          />
          {form.formState.errors.context_window_tokens && (
            <p className="text-xs text-destructive">
              {form.formState.errors.context_window_tokens.message}
            </p>
          )}
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`provider-output-reserve-${provider.id}`}>{t('fields.outputReservePercent')}</Label>
          <Input
            id={`provider-output-reserve-${provider.id}`}
            type="number"
            min={1}
            max={90}
            {...form.register('context_output_reserve_percent', { valueAsNumber: true })}
          />
          {form.formState.errors.context_output_reserve_percent && (
            <p className="text-xs text-destructive">
              {form.formState.errors.context_output_reserve_percent.message}
            </p>
          )}
        </div>
      </div>

      {kind === 'openai-compatible' && (
        <ReasoningPassbackControl
          value={form.watch('reasoning_passback')}
          onChange={(value) => form.setValue('reasoning_passback', value)}
        />
      )}

      <div className="space-y-1.5">
        <Label htmlFor={`provider-desc-${provider.id}`}>{t('fields.descriptionOptional')}</Label>
        <Textarea id={`provider-desc-${provider.id}`} rows={2} {...form.register('description')} />
      </div>

      {submitError && (
        <p className="text-sm text-destructive" role="alert">
          {submitError}
        </p>
      )}
      <Button type="submit" disabled={update.isPending}>
        {update.isPending ? t('actions.saving') : t('form.saveChanges')}
      </Button>
    </form>
  )
}
