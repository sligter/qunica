import { useState } from 'react'

import { EditProviderForm } from '@/components/providers/EditProviderForm'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import { useDeleteProvider } from '@/hooks/useProviders'
import type { LLMProviderRead } from '@/types/api'

interface ProviderDetailDialogProps {
  provider: LLMProviderRead | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function ProviderDetailDialog({
  provider,
  open,
  onOpenChange,
}: ProviderDetailDialogProps) {
  const [editing, setEditing] = useState(false)
  const del = useDeleteProvider()

  if (!provider) return null

  const onDelete = async () => {
    if (!confirm(`Delete provider "${provider.name}"? Agents using it will fall back to defaults.`)) {
      return
    }
    await del.mutateAsync(provider.id)
    onOpenChange(false)
  }

  const handleOpenChange = (value: boolean) => {
    if (!value) setEditing(false)
    onOpenChange(value)
  }

  if (editing) {
    return (
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent className="max-w-lg max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Edit {provider.name}</DialogTitle>
          </DialogHeader>
          <EditProviderForm provider={provider} onSaved={() => setEditing(false)} />
        </DialogContent>
      </Dialog>
    )
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <div className="flex items-center justify-between pr-8">
            <DialogTitle>{provider.name}</DialogTitle>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={() => setEditing(true)}>
                Edit
              </Button>
              <Button
                variant="destructive"
                size="sm"
                onClick={onDelete}
                disabled={del.isPending}
              >
                {del.isPending ? 'Deleting...' : 'Delete'}
              </Button>
            </div>
          </div>
        </DialogHeader>

        <Separator />

        <div className="grid grid-cols-2 gap-4 text-sm">
          <Field label="Kind" value={provider.kind} />
          <Field label="Default model" value={provider.default_model} />
          <Field
            label="Context window"
            value={
              provider.context_window_tokens !== null
                ? provider.context_window_tokens.toLocaleString()
                : 'Auto'
            }
          />
          <Field
            label="Output reserve"
            value={
              provider.context_output_reserve_ratio !== null
                ? `${Math.round(provider.context_output_reserve_ratio * 100)}%`
                : '30%'
            }
          />
          <Field label="Base URL" value={provider.base_url ?? '-'} />
          <Field label="API key" value={provider.api_key_masked} mono />
          <Field label="Status">
            <Badge variant={provider.status === 'active' ? 'default' : 'secondary'}>
              {provider.status}
            </Badge>
          </Field>
        </div>

        {provider.description && (
          <>
            <Separator />
            <section className="space-y-2">
              <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                Description
              </h3>
              <p className="whitespace-pre-wrap text-sm">{provider.description}</p>
            </section>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}

function Field({
  label,
  value,
  mono,
  children,
}: {
  label: string
  value?: string
  mono?: boolean
  children?: React.ReactNode
}) {
  return (
    <div>
      <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </h3>
      {children ?? <p className={mono ? 'font-mono text-sm' : 'text-sm'}>{value}</p>}
    </div>
  )
}
