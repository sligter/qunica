import { useEffect, useMemo, useRef, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { useTranslation } from 'react-i18next'

import {
  ProviderModelsField,
  type ProviderModelDraft,
} from '@/components/providers/ProviderModelsField'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import {
  useProviderModels,
  useTestProviderModel,
  useUpdateProvider,
} from '@/hooks/useProviders'
import { ApiError } from '@/lib/api-v2/client'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'
import { cn } from '@/lib/utils'
import type { LLMProviderRead, ProviderKind } from '@/types/api'

function createSchema(required: string) { return z.object({
  name: z.string().min(1, required).max(100),
  kind: z.enum(['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']),
  base_url: z.string().optional(),
  api_key: z.string().optional(),
  description: z.string().optional(),
}) }

type FormValues = z.infer<ReturnType<typeof createSchema>>

interface EditProviderFormProps {
  provider: LLMProviderRead
  onSaved?: (providerId: string) => void
  onSavingChange?: (saving: boolean) => void
}

export const EDIT_PROVIDER_FORM_ID = 'edit-provider-form'

const KIND_OPTIONS: ProviderKind[] = ['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']
const KIND_KEYS: Record<ProviderKind, 'openai' | 'anthropic' | 'anthropicCompatible' | 'gemini'> = { 'openai-compatible': 'openai', anthropic: 'anthropic', 'anthropic-compatible': 'anthropicCompatible', gemini: 'gemini' }

function baseUrlPlaceholder(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') {
    return 'https://api.anthropic.com'
  }
  if (kind === 'gemini') return 'https://generativelanguage.googleapis.com/v1beta'
  return 'https://api.openai.com/v1'
}

export function EditProviderForm({
  provider,
  onSaved,
  onSavingChange,
}: EditProviderFormProps) {
  const { t, i18n } = useTranslation('providers')
  const update = useUpdateProvider(provider.id)
  const catalog = useProviderModels(provider.id)
  const testProviderModel = useTestProviderModel()
  const [submitError, setSubmitError] = useState<LocalizedError | null>(null)
  const [models, setModels] = useState<ProviderModelDraft[]>(
    (provider.models ?? [{
      id: provider.default_model,
      context_window_tokens: provider.context_window_tokens,
      context_output_reserve_ratio: provider.context_output_reserve_ratio,
    }]).map((model) => ({
      id: model.id,
      context_window_tokens: model.context_window_tokens ?? undefined,
      context_output_reserve_percent:
        model.context_output_reserve_ratio !== null
          ? Math.round(model.context_output_reserve_ratio * 100)
          : 30,
      reasoning_passback: model.reasoning_passback ?? provider.reasoning_passback,
    })),
  )
  const [defaultModel, setDefaultModel] = useState(provider.default_model)
  const schema = useMemo(() => createSchema(t('validation.required')), [t])
  const validationLanguage = useRef(i18n.resolvedLanguage)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: provider.name,
      kind: provider.kind,
      base_url: provider.base_url ?? '',
      api_key: '',
      description: provider.description ?? '',
    },
  })

  useEffect(() => {
    if (validationLanguage.current === i18n.resolvedLanguage) return
    validationLanguage.current = i18n.resolvedLanguage
    if (Object.keys(form.formState.errors).length > 0) {
      void form.trigger()
    }
  }, [form, i18n.resolvedLanguage])

  useEffect(() => {
    onSavingChange?.(update.isPending)
  }, [onSavingChange, update.isPending])

  const kind = form.watch('kind')

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    const normalizedModels = models.map((model) => ({ ...model, id: model.id.trim() }))
    if (normalizedModels.some((model) => !model.id)) {
      setSubmitError(messageError(t('models.required')))
      return
    }
    if (new Set(normalizedModels.map((model) => model.id)).size !== normalizedModels.length) {
      setSubmitError(messageError(t('models.duplicate')))
      return
    }
    const resolvedDefault = normalizedModels.some((model) => model.id === defaultModel)
      ? defaultModel
      : normalizedModels[0].id
    try {
      const updated = await update.mutateAsync({
        name: values.name,
        kind: values.kind,
        base_url: values.base_url || null,
        api_key: values.api_key ? values.api_key : undefined,
        default_model: resolvedDefault,
        models: normalizedModels.map((model) => ({
          id: model.id,
          context_window_tokens: model.context_window_tokens ?? null,
          context_output_reserve_ratio: model.context_output_reserve_percent / 100,
          reasoning_passback: model.reasoning_passback,
        })),
        description: values.description || null,
      })
      onSaved?.(updated.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? messageError(err.message) : translatedError('errors.network'))
    }
  })

  return (
    <form id={EDIT_PROVIDER_FORM_ID} onSubmit={onSubmit} className="space-y-4">
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
                <span className="text-2xs leading-snug text-muted-foreground">
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
        <p className="text-2xs text-muted-foreground">
          {t('form.keyMaskedHint')}
        </p>
      </div>

      <ProviderModelsField
        models={models}
        defaultModel={defaultModel}
        catalog={catalog.data}
        isLoadingCatalog={catalog.isFetching}
        catalogError={catalog.isError}
        showReasoningPassback={kind === 'openai-compatible'}
        onChange={setModels}
        onDefaultChange={setDefaultModel}
        onRefreshCatalog={() => void catalog.refetch()}
        onTestModel={(model) => {
          const values = form.getValues()
          return testProviderModel.mutateAsync({
            provider_id: provider.id,
            kind: values.kind,
            base_url: values.base_url || null,
            api_key: values.api_key?.trim() || undefined,
            model,
          })
        }}
      />

      <div className="space-y-1.5">
        <Label htmlFor={`provider-desc-${provider.id}`}>{t('fields.descriptionOptional')}</Label>
        <Textarea id={`provider-desc-${provider.id}`} rows={2} {...form.register('description')} />
      </div>

      {localizedErrorText(submitError, t) && (
        <p className="text-sm text-destructive" role="alert">
          {localizedErrorText(submitError, t)}
        </p>
      )}
    </form>
  )
}
