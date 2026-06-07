import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import { ReasoningPassbackControl } from '@/components/providers/ReasoningPassbackControl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useCreateProvider } from '@/hooks/useProviders'
import { ApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type { ProviderKind } from '@/types/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  kind: z.enum(['openai-compatible', 'anthropic', 'anthropic-compatible', 'gemini']),
  base_url: z.string().optional(),
  api_key: z.string().min(1, 'Required'),
  default_model: z.string().min(1, 'Required'),
  context_window_tokens: z.preprocess(
    (value) => (value === '' || Number.isNaN(value) ? undefined : value),
    z.number().int().min(1).optional(),
  ),
  context_output_reserve_percent: z.number().min(1).max(90),
  description: z.string().optional(),
  reasoning_passback: z.boolean(),
})

type FormValues = z.infer<typeof schema>

interface CreateProviderFormProps {
  onCreated?: (newProviderId: string) => void
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

function baseUrlHint(kind: ProviderKind): string {
  if (kind === 'openai-compatible') {
    return 'Examples: https://api.deepseek.com/v1, https://token-plan-cn.xiaomimimo.com/v1.'
  }
  if (kind === 'anthropic-compatible') {
    return 'Use a gateway base URL exposing Anthropic-compatible /v1/messages and /v1/models routes.'
  }
  if (kind === 'anthropic') return 'Leave empty for default api.anthropic.com.'
  return 'Leave empty for default Google AI endpoint.'
}

function modelPlaceholder(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') return 'claude-sonnet-4-5'
  if (kind === 'gemini') return 'gemini-2.5-pro or gemini-2.5-flash'
  return 'gpt-4o-mini or mimo-v2.5-pro or deepseek-chat'
}

export function CreateProviderForm({ onCreated }: CreateProviderFormProps = {}) {
  const create = useCreateProvider()
  const [submitError, setSubmitError] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      kind: 'openai-compatible',
      base_url: '',
      api_key: '',
      default_model: '',
      context_window_tokens: undefined,
      context_output_reserve_percent: 30,
      description: '',
      reasoning_passback: false,
    },
  })

  const kind = form.watch('kind')

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      const created = await create.mutateAsync({
        name: values.name,
        kind: values.kind,
        base_url: values.base_url || null,
        api_key: values.api_key,
        default_model: values.default_model,
        context_window_tokens: values.context_window_tokens ?? null,
        context_output_reserve_ratio: values.context_output_reserve_percent / 100,
        description: values.description || null,
        reasoning_passback: values.reasoning_passback,
      })
      form.reset()
      onCreated?.(created.id)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : 'Network error')
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="provider-name">Name</Label>
        <Input
          id="provider-name"
          placeholder="e.g. MiMo prod"
          {...form.register('name')}
        />
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
                onClick={() => form.setValue('kind', opt.value)}
                className={cn(
                  'flex min-h-20 flex-col items-start gap-1 rounded-md border px-3 py-2 text-left transition-colors',
                  checked
                    ? 'border-primary bg-primary/10'
                    : 'border-border hover:bg-card-hover',
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
        <Label htmlFor="provider-base-url">Base URL (optional)</Label>
        <Input
          id="provider-base-url"
          placeholder={baseUrlPlaceholder(kind)}
          {...form.register('base_url')}
        />
        <p className="text-[11px] text-muted-foreground">{baseUrlHint(kind)}</p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="provider-api-key">API key</Label>
        <Input
          id="provider-api-key"
          type="password"
          placeholder="sk-..."
          {...form.register('api_key')}
        />
        {form.formState.errors.api_key && (
          <p className="text-xs text-red-600">
            {form.formState.errors.api_key.message}
          </p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="provider-model">Default model</Label>
        <Input
          id="provider-model"
          placeholder={modelPlaceholder(kind)}
          {...form.register('default_model')}
        />
        {form.formState.errors.default_model && (
          <p className="text-xs text-red-600">
            {form.formState.errors.default_model.message}
          </p>
        )}
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="provider-context-window">Context window tokens</Label>
          <Input
            id="provider-context-window"
            type="number"
            min={1}
            placeholder="Auto from model"
            {...form.register('context_window_tokens', { valueAsNumber: true })}
          />
          {form.formState.errors.context_window_tokens && (
            <p className="text-xs text-red-600">
              {form.formState.errors.context_window_tokens.message}
            </p>
          )}
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="provider-output-reserve">Output reserve %</Label>
          <Input
            id="provider-output-reserve"
            type="number"
            min={1}
            max={90}
            {...form.register('context_output_reserve_percent', { valueAsNumber: true })}
          />
          {form.formState.errors.context_output_reserve_percent && (
            <p className="text-xs text-red-600">
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
        <Label htmlFor="provider-desc">Description (optional)</Label>
        <Textarea
          id="provider-desc"
          rows={2}
          {...form.register('description')}
        />
      </div>

      {submitError && (
        <p className="text-sm text-red-600" role="alert">
          {submitError}
        </p>
      )}
      <Button type="submit" disabled={create.isPending}>
        {create.isPending ? 'Saving...' : 'Add provider'}
      </Button>
    </form>
  )
}
