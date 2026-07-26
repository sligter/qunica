import { useEffect, useMemo, useRef, useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { useTranslation } from 'react-i18next'

import {
  ProviderModelsField,
  type ProviderModelDraft,
} from '@/components/providers/ProviderModelsField'
import { ReasoningPassbackControl } from '@/components/providers/ReasoningPassbackControl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useCreateProvider, useDiscoverProviderModels } from '@/hooks/useProviders'
import { ApiError } from '@/lib/api-v2/client'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'
import { cn } from '@/lib/utils'
import type { ProviderKind } from '@/types/api'

function createSchema(required: string) { return z.object({
  name: z.string().min(1, required).max(100),
  kind: z.enum(['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']),
  base_url: z.string().optional(),
  api_key: z.string().min(1, required),
  description: z.string().optional(),
  reasoning_passback: z.boolean(),
}) }

type FormValues = z.infer<ReturnType<typeof createSchema>>

interface CreateProviderFormProps {
  onCreated?: (newProviderId: string) => void
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

export function CreateProviderForm({ onCreated }: CreateProviderFormProps = {}) {
  const { t, i18n } = useTranslation('providers')
  const create = useCreateProvider()
  const discoverModels = useDiscoverProviderModels()
  const [submitError, setSubmitError] = useState<LocalizedError | null>(null)
  const [models, setModels] = useState<ProviderModelDraft[]>([
    { id: '', context_window_tokens: undefined, context_output_reserve_percent: 30 },
  ])
  const [defaultModel, setDefaultModel] = useState('')
  const schema = useMemo(() => createSchema(t('validation.required')), [t])
  const validationLanguage = useRef(i18n.resolvedLanguage)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      kind: 'openai-compatible',
      base_url: '',
      api_key: '',
      description: '',
      reasoning_passback: false,
    },
  })

  useEffect(() => {
    if (validationLanguage.current === i18n.resolvedLanguage) return
    validationLanguage.current = i18n.resolvedLanguage
    if (Object.keys(form.formState.errors).length > 0) {
      void form.trigger()
    }
  }, [form, i18n.resolvedLanguage])

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
      const created = await create.mutateAsync({
        name: values.name,
        kind: values.kind,
        base_url: values.base_url || null,
        api_key: values.api_key,
        default_model: resolvedDefault,
        models: normalizedModels.map((model) => ({
          id: model.id,
          context_window_tokens: model.context_window_tokens ?? null,
          context_output_reserve_ratio: model.context_output_reserve_percent / 100,
        })),
        description: values.description || null,
        reasoning_passback: values.reasoning_passback,
      })
      form.reset()
      onCreated?.(created.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? messageError(err.message) : translatedError('errors.network'))
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="provider-name">{t('fields.name')}</Label>
        <Input
          id="provider-name"
          placeholder={t('form.namePlaceholder')}
          {...form.register('name')}
        />
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
                onClick={() => form.setValue('kind', opt)}
                className={cn(
                  'flex min-h-20 flex-col items-start gap-1 rounded-md border px-3 py-2 text-left transition-colors',
                  checked
                    ? 'border-primary bg-primary/10'
                    : 'border-border hover:bg-card-hover',
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
        <Label htmlFor="provider-base-url">{t('fields.baseUrlOptional')}</Label>
        <Input
          id="provider-base-url"
          placeholder={baseUrlPlaceholder(kind)}
          {...form.register('base_url')}
        />
        <p className="text-[11px] text-muted-foreground">{t(`kinds.${KIND_KEYS[kind]}.baseHint`)}</p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="provider-api-key">{t('fields.apiKey')}</Label>
        <Input
          id="provider-api-key"
          type="password"
          placeholder="sk-..."
          {...form.register('api_key')}
        />
        {form.formState.errors.api_key && (
          <p className="text-xs text-destructive">
            {form.formState.errors.api_key.message}
          </p>
        )}
      </div>

      <ProviderModelsField
        models={models}
        defaultModel={defaultModel}
        catalog={discoverModels.data}
        isLoadingCatalog={discoverModels.isPending}
        catalogError={discoverModels.isError}
        onChange={setModels}
        onDefaultChange={setDefaultModel}
        onRefreshCatalog={() => {
          const values = form.getValues()
          if (!values.api_key) {
            void form.trigger('api_key')
            return
          }
          discoverModels.mutate({
            kind: values.kind,
            base_url: values.base_url || null,
            api_key: values.api_key,
            default_model: defaultModel || null,
          })
        }}
      />

      {kind === 'openai-compatible' && (
        <ReasoningPassbackControl
          value={form.watch('reasoning_passback')}
          onChange={(value) => form.setValue('reasoning_passback', value)}
        />
      )}

      <div className="space-y-1.5">
        <Label htmlFor="provider-desc">{t('fields.descriptionOptional')}</Label>
        <Textarea
          id="provider-desc"
          rows={2}
          {...form.register('description')}
        />
      </div>

      {localizedErrorText(submitError, t) && (
        <p className="text-sm text-destructive" role="alert">
          {localizedErrorText(submitError, t)}
        </p>
      )}
      <Button type="submit" disabled={create.isPending}>
        {create.isPending ? t('actions.saving') : t('form.add')}
      </Button>
    </form>
  )
}
