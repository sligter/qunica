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
  const del = useDeleteProvider()

  if (!provider) return null

  const onDelete = async () => {
    if (!confirm(`Delete provider "${provider.name}"? Agents using it will fall back to defaults.`)) {
      return
    }
    await del.mutateAsync(provider.id)
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <div className="flex items-center justify-between pr-8">
            <DialogTitle>{provider.name}</DialogTitle>
            <Button
              variant="destructive"
              size="sm"
              onClick={onDelete}
              disabled={del.isPending}
            >
              {del.isPending ? 'Deleting…' : 'Delete'}
            </Button>
          </div>
        </DialogHeader>

        <Separator />

        <div className="grid grid-cols-2 gap-4 text-sm">
          <Field label="Kind" value={provider.kind} />
          <Field label="Default model" value={provider.default_model} />
          <Field label="Base URL" value={provider.base_url ?? '—'} />
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
