import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'

import { EditProviderForm } from '@/components/providers/EditProviderForm'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { useDeleteProvider, useProvider } from '@/hooks/useProviders'

export function ProviderDetailPage() {
  const { providerId } = useParams<{ providerId: string }>()
  const provider = useProvider(providerId)
  const del = useDeleteProvider()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)

  if (provider.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }
  if (provider.error) {
    return (
      <div className="p-6 text-sm text-destructive">
        Failed to load: {String(provider.error)}
      </div>
    )
  }
  if (!provider.data) {
    return <div className="p-6 text-sm text-muted-foreground">Provider not found.</div>
  }

  const p = provider.data

  if (editing) {
    return (
      <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
        <div className="mx-auto w-full max-w-2xl space-y-4 p-8">
          <header className="flex items-baseline justify-between gap-4">
            <h1 className="font-serif text-xl font-semibold tracking-tight">
              Edit {p.name}
            </h1>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              Cancel
            </Button>
          </header>
          <EditProviderForm provider={p} onSaved={() => setEditing(false)} />
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-2xl space-y-6 p-8">
        <header className="flex items-baseline justify-between gap-4">
          <div className="space-y-1">
            <h1 className="font-serif text-xl font-semibold tracking-tight">{p.name}</h1>
            <p className="text-sm text-muted-foreground">
              {p.kind} · {p.default_model}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => setEditing(true)}>
              Edit
            </Button>
            <Button
              variant="destructive"
              size="sm"
              onClick={() => setConfirmOpen(true)}
              disabled={del.isPending}
            >
              {del.isPending ? 'Deleting…' : 'Delete'}
            </Button>
          </div>
        </header>

        <section className="grid grid-cols-2 gap-4 text-sm">
          <Field label="Kind" value={p.kind} />
          <Field label="Default model" value={p.default_model} />
          <Field
            label="Context window"
            value={
              p.context_window_tokens !== null
                ? p.context_window_tokens.toLocaleString()
                : 'Auto'
            }
          />
          <Field
            label="Output reserve"
            value={
              p.context_output_reserve_ratio !== null
                ? `${Math.round(p.context_output_reserve_ratio * 100)}%`
                : '30%'
            }
          />
          <Field label="Base URL" value={p.base_url ?? '-'} />
          <Field label="API key" value={p.api_key_masked} mono />
          <Field label="Status">
            <Badge variant={p.status === 'active' ? 'default' : 'secondary'}>
              {p.status}
            </Badge>
          </Field>
        </section>

        {p.description && (
          <section className="space-y-2">
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Description
            </h3>
            <p className="whitespace-pre-wrap text-sm">{p.description}</p>
          </section>
        )}
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={`Delete provider "${p.name}"?`}
        description="Agents using it will fall back to defaults."
        confirmLabel="Delete"
        destructive
        onConfirm={async () => {
          await del.mutateAsync(p.id)
          void navigate('/providers')
        }}
      />
    </div>
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
