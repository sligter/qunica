import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useCreateAgent } from '@/hooks/useCreateAgent'
import { ApiError } from '@/lib/api'

const schema = z.object({
  name: z.string().min(1, 'Required').max(100),
  description: z.string().optional(),
  system_prompt: z.string().min(1, 'Required'),
})

type FormValues = z.infer<typeof schema>

interface CreateAgentFormProps {
  onCreated?: (newAgentId: string) => void
}

export function CreateAgentForm({ onCreated }: CreateAgentFormProps = {}) {
  const createAgent = useCreateAgent()
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [submittedName, setSubmittedName] = useState<string | null>(null)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: '', description: '', system_prompt: '' },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    setSubmittedName(null)
    try {
      const created = await createAgent.mutateAsync({
        name: values.name,
        description: values.description,
        system_prompt: values.system_prompt,
      })
      form.reset()
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
        <Textarea
          id="agent-system-prompt"
          rows={5}
          placeholder="You are a concise assistant. Always end with the word DONE."
          {...form.register('system_prompt')}
        />
        {form.formState.errors.system_prompt && (
          <p className="text-xs text-red-600">
            {form.formState.errors.system_prompt.message}
          </p>
        )}
      </div>
      {submitError && (
        <p className="text-sm text-red-600" role="alert">
          {submitError}
        </p>
      )}
      {submittedName && (
        <p className="text-sm text-green-700">Created agent: {submittedName}</p>
      )}
      <Button type="submit" disabled={createAgent.isPending}>
        {createAgent.isPending ? 'Creating…' : 'Create agent'}
      </Button>
    </form>
  )
}
