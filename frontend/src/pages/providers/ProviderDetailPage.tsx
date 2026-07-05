import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'

import { EditProviderForm } from '@/components/providers/EditProviderForm'
import { DetailShell } from '@/components/layout/DetailShell'
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
      <DetailShell
        title={`Edit ${p.name}`}
        actions={
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
            Cancel
          </Button>
        }
      >
        <EditProviderForm provider={p} onSaved={() => setEditing(false)} />
      </DetailShell>
    )
  }

  return (
    <DetailShell
      title={p.name}
      subtitle={`${p.kind} · ${p.default_model}`}
      actions={
        <>
          <Button variant="ghost" size="sm" onClick={() => setEditing(true)}>
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
        </>
      }
    >
      <div className="space-y-8">
        <section className="grid grid-cols-1 gap-x-8 gap-y-4 text-sm sm:grid-cols-2 xl:grid-cols-4">
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
          void navigate('/settings/providers')
        }}
      />
    </DetailShell>
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
      {children ?? (
        <p className={mono ? 'mt-1 font-mono text-sm' : 'mt-1 text-sm'}>{value}</p>
      )}
    </div>
  )
}
