import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

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
  kind: z.enum(['openai-compatible', 'anthropic', 'gemini']),
  base_url: z.string().optional(),
  api_key: z.string().min(1, 'Required'),
  default_model: z.string().min(1, 'Required'),
  description: z.string().optional(),
})

type FormValues = z.infer<typeof schema>

interface CreateProviderFormProps {
  onCreated?: (newProviderId: string) => void
}

const KIND_OPTIONS: { value: ProviderKind; label: string; hint: string }[] = [
  {
    value: 'openai-compatible',
    label: 'OpenAI-compatible',
    hint: 'OpenAI · DeepSeek · Qwen · MiMo · Together · OpenRouter',
  },
  {
    value: 'anthropic',
    label: 'Anthropic',
    hint: 'Claude (api.anthropic.com)',
  },
  {
    value: 'gemini',
    label: 'Gemini',
    hint: 'Google Gemini (generativelanguage.googleapis.com)',
  },
]

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
      description: '',
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
        description: values.description || null,
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
        <div className="grid grid-cols-2 gap-2">
          {KIND_OPTIONS.map((opt) => {
            const checked = kind === opt.value
            return (
              <button
                type="button"
                key={opt.value}
                onClick={() => form.setValue('kind', opt.value)}
                className={cn(
                  'flex flex-col items-start gap-1 rounded-md border px-3 py-2 text-left transition-colors',
                  checked
                    ? 'border-primary bg-primary/10'
                    : 'border-border hover:bg-card-hover',
                )}
              >
                <span className="text-sm font-medium">{opt.label}</span>
                <span className="text-[11px] text-muted-foreground">{opt.hint}</span>
              </button>
            )
          })}
        </div>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="provider-base-url">Base URL (optional)</Label>
        <Input
          id="provider-base-url"
          placeholder={
            kind === 'anthropic'
              ? 'https://api.anthropic.com'
              : kind === 'gemini'
                ? 'https://generativelanguage.googleapis.com/v1beta'
                : 'https://api.openai.com/v1'
          }
          {...form.register('base_url')}
        />
        <p className="text-[11px] text-muted-foreground">
          {kind === 'openai-compatible' && (
            <>Examples: <code>https://api.deepseek.com/v1</code>, <code>https://token-plan-cn.xiaomimimo.com/v1</code>.</>
          )}
          {kind === 'anthropic' && 'Leave empty for default api.anthropic.com.'}
          {kind === 'gemini' && 'Leave empty for default Google AI endpoint.'}
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="provider-api-key">API key</Label>
        <Input
          id="provider-api-key"
          type="password"
          placeholder="sk-…"
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
          placeholder={
            kind === 'anthropic'
              ? 'claude-sonnet-4-5'
              : kind === 'gemini'
                ? 'gemini-2.5-pro · gemini-2.5-flash'
                : 'gpt-4o-mini · mimo-v2.5-pro · deepseek-chat'
          }
          {...form.register('default_model')}
        />
        {form.formState.errors.default_model && (
          <p className="text-xs text-red-600">
            {form.formState.errors.default_model.message}
          </p>
        )}
      </div>

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
        {create.isPending ? 'Saving…' : 'Add provider'}
      </Button>
    </form>
  )
}
