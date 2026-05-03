import { useNavigate, useParams } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import { useDeleteProvider, useProvider } from '@/hooks/useProviders'

export function ProviderDetailRightPane() {
  const { providerId } = useParams<{ providerId: string }>()
  const provider = useProvider(providerId)
  const del = useDeleteProvider()
  const navigate = useNavigate()

  if (provider.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }
  if (provider.error) {
    return (
      <div className="p-6 text-sm text-red-600">
        Failed to load: {String(provider.error)}
      </div>
    )
  }
  if (!provider.data) {
    return <div className="p-6 text-sm text-muted-foreground">Provider not found.</div>
  }

  const p = provider.data

  const onDelete = async () => {
    if (!confirm(`Delete provider "${p.name}"? Agents using it will fall back to defaults.`)) {
      return
    }
    await del.mutateAsync(p.id)
    void navigate('/providers')
  }

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-2xl space-y-6 p-8">
        <header className="flex items-baseline justify-between gap-4">
          <div className="space-y-1">
            <h1 className="text-xl font-semibold tracking-tight">{p.name}</h1>
            <p className="text-sm text-muted-foreground">
              {p.kind} · {p.default_model}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onDelete}
            disabled={del.isPending}
          >
            {del.isPending ? 'Deleting…' : 'Delete'}
          </Button>
        </header>

        <section className="grid grid-cols-2 gap-4 text-sm">
          <Field label="Kind" value={p.kind} />
          <Field label="Default model" value={p.default_model} />
          <Field label="Base URL" value={p.base_url ?? '—'} />
          <Field label="API key" value={p.api_key_masked} mono />
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
    </div>
  )
}

function Field({
  label,
  value,
  mono,
}: {
  label: string
  value: string
  mono?: boolean
}) {
  return (
    <div>
      <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </h3>
      <p className={mono ? 'font-mono text-sm' : 'text-sm'}>{value}</p>
    </div>
  )
}
