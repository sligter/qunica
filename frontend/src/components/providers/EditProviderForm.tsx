import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import { ReasoningPassbackControl } from '@/components/providers/ReasoningPassbackControl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useUpdateProvider } from '@/hooks/useProviders'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type { LLMProviderRead, ProviderKind } from '@/types/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  kind: z.enum(['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']),
  base_url: z.string().optional(),
  api_key: z.string().optional(),
  default_model: z.string().min(1, 'Required'),
  description: z.string().optional(),
  reasoning_passback: z.boolean(),
})

type FormValues = z.infer<typeof schema>

interface EditProviderFormProps {
  provider: LLMProviderRead
  onSaved?: (providerId: string) => void
}

const KIND_OPTIONS: { value: ProviderKind; label: string; hint: string }[] = [
  {
    value: 'openai-compatible',
    label: 'OpenAI-compatible',
    hint: 'OpenAI, DeepSeek, Qwen, MiMo, Together, OpenRouter',
  },
  {
    value: 'anthropic',
    label: 'Anthropic',
    hint: 'Claude direct API at api.anthropic.com',
  },
  {
    value: 'anthropic-compatible',
    label: 'Anthropic-compatible',
    hint: 'Claude-compatible gateways using Anthropic message format',
  },
  {
    value: 'gemini',
    label: 'Gemini',
    hint: 'Google Gemini at generativelanguage.googleapis.com',
  },
]

function baseUrlPlaceholder(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') {
    return 'https://api.anthropic.com'
  }
  if (kind === 'gemini') return 'https://generativelanguage.googleapis.com/v1beta'
  return 'https://api.openai.com/v1'
}

function modelPlaceholder(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') return 'claude-sonnet-4-5'
  if (kind === 'gemini') return 'gemini-2.5-pro or gemini-2.5-flash'
  return 'gpt-4o-mini or mimo-v2.5-pro or deepseek-chat'
}

export function EditProviderForm({ provider, onSaved }: EditProviderFormProps) {
  const update = useUpdateProvider(provider.id)
  const [submitError, setSubmitError] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: provider.name,
      kind: provider.kind,
      base_url: provider.base_url ?? '',
      api_key: '',
      default_model: provider.default_model,
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
        description: values.description || null,
        reasoning_passback: values.reasoning_passback,
      })
      onSaved?.(updated.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : 'Network error')
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor={`provider-name-${provider.id}`}>Name</Label>
        <Input id={`provider-name-${provider.id}`} {...form.register('name')} />
        {form.formState.errors.name && (
          <p className="text-xs text-red-600">{form.formState.errors.name.message}</p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label>Kind</Label>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {KIND_OPTIONS.map((opt) => {
            const checked = kind === opt.value
            return (
              <button
                type="button"
                key={opt.value}
                onClick={() => form.setValue('kind', opt.value, { shouldDirty: true })}
                className={cn(
                  'flex min-h-20 flex-col items-start gap-1 rounded-md border px-3 py-2 text-left transition-colors',
                  checked ? 'border-primary bg-primary/10' : 'border-border hover:bg-card-hover',
                )}
              >
                <span className="text-sm font-medium">{opt.label}</span>
                <span className="text-[11px] leading-snug text-muted-foreground">
                  {opt.hint}
                </span>
              </button>
            )
          })}
        </div>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor={`provider-base-url-${provider.id}`}>Base URL (optional)</Label>
        <Input
          id={`provider-base-url-${provider.id}`}
          placeholder={baseUrlPlaceholder(kind)}
          {...form.register('base_url')}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor={`provider-api-key-${provider.id}`}>API key</Label>
        <Input
          id={`provider-api-key-${provider.id}`}
          type="password"
          placeholder={`Leave blank to keep ${provider.api_key_masked}`}
          {...form.register('api_key')}
        />
        <p className="text-[11px] text-muted-foreground">
          Existing keys are masked. Enter a new key only when rotating credentials.
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor={`provider-model-${provider.id}`}>Default model</Label>
        <Input
          id={`provider-model-${provider.id}`}
          placeholder={modelPlaceholder(kind)}
          {...form.register('default_model')}
        />
        {form.formState.errors.default_model && (
          <p className="text-xs text-red-600">
            {form.formState.errors.default_model.message}
          </p>
        )}
      </div>

      {kind === 'openai-compatible' && (
        <ReasoningPassbackControl
          value={form.watch('reasoning_passback')}
          onChange={(value) => form.setValue('reasoning_passback', value)}
        />
      )}

      <div className="space-y-1.5">
        <Label htmlFor={`provider-desc-${provider.id}`}>Description (optional)</Label>
        <Textarea id={`provider-desc-${provider.id}`} rows={2} {...form.register('description')} />
      </div>

      {submitError && (
        <p className="text-sm text-red-600" role="alert">
          {submitError}
        </p>
      )}
      <Button type="submit" disabled={update.isPending}>
        {update.isPending ? 'Saving...' : 'Save changes'}
      </Button>
    </form>
  )
}
