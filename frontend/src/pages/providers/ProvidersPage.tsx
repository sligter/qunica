import { useState } from 'react'
import { Plug, Plus } from 'lucide-react'

import { CreateProviderDialog } from '@/components/providers/CreateProviderDialog'
import { ProviderDetailDialog } from '@/components/providers/ProviderDetailDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { useProviders } from '@/hooks/useProviders'
import type { LLMProviderRead, ProviderKind } from '@/types/api'

function kindColor(kind: ProviderKind) {
  if (kind === 'anthropic') return 'bg-orange-500/90 text-white'
  if (kind === 'anthropic-compatible') return 'bg-amber-500/90 text-white'
  if (kind === 'gemini') return 'bg-blue-500/90 text-white'
  return 'bg-violet-500/90 text-white'
}

function kindInitial(kind: ProviderKind): string {
  if (kind === 'anthropic') return 'A'
  if (kind === 'anthropic-compatible') return 'C'
  if (kind === 'gemini') return 'G'
  return 'O'
}

export function ProvidersPage() {
  const providers = useProviders()
  const [createOpen, setCreateOpen] = useState(false)
  const [selected, setSelected] = useState<LLMProviderRead | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-6">
        <div className="flex items-center gap-2">
          <Plug className="h-5 w-5 text-muted-foreground" />
          <h1 className="text-base font-semibold tracking-tight">Providers</h1>
          {providers.data && (
            <span className="text-xs text-muted-foreground">({providers.data.length})</span>
          )}
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <Plus className="mr-1 h-4 w-4" />
          New Provider
        </Button>
      </header>

      <div className="flex-1 overflow-y-auto p-6">
        {providers.isLoading && (
          <p className="text-sm text-muted-foreground">Loading providers...</p>
        )}
        {providers.error && (
          <p className="text-sm text-red-600">Failed to load providers.</p>
        )}
        {providers.data && providers.data.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-3 py-20 text-center">
            <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <Plug className="h-7 w-7" />
            </div>
            <h2 className="text-base font-medium">No providers yet</h2>
            <p className="max-w-sm text-sm text-muted-foreground">
              Register an OpenAI-compatible, Anthropic-compatible, Anthropic, or Gemini endpoint.
            </p>
            <Button size="sm" onClick={() => setCreateOpen(true)}>
              <Plus className="mr-1 h-4 w-4" />
              Add Provider
            </Button>
          </div>
        )}

        {providers.data && providers.data.length > 0 && (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {providers.data.map((p) => (
              <Card
                key={p.id}
                className="cursor-pointer transition-shadow hover:shadow-md"
                onClick={() => {
                  setSelected(p)
                  setDetailOpen(true)
                }}
              >
                <CardHeader className="flex flex-row items-start gap-3 space-y-0 pb-3">
                  <Avatar className="h-10 w-10 shrink-0">
                    <AvatarFallback className={kindColor(p.kind)}>
                      {kindInitial(p.kind)}
                    </AvatarFallback>
                  </Avatar>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <h3 className="truncate text-sm font-semibold">{p.name}</h3>
                    <p className="line-clamp-1 text-xs text-muted-foreground">
                      {p.default_model}
                    </p>
                  </div>
                </CardHeader>
                <CardContent className="space-y-2">
                  <div className="flex flex-wrap gap-1.5">
                    <Badge variant="outline" className="text-[10px]">
                      {p.kind}
                    </Badge>
                    <Badge
                      variant={p.status === 'active' ? 'default' : 'secondary'}
                      className="text-[10px]"
                    >
                      {p.status}
                    </Badge>
                  </div>
                  {p.base_url && (
                    <p className="truncate text-[10px] text-muted-foreground">{p.base_url}</p>
                  )}
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </div>

      <CreateProviderDialog open={createOpen} onOpenChange={setCreateOpen} />
      <ProviderDetailDialog
        provider={selected}
        open={detailOpen}
        onOpenChange={(v) => {
          setDetailOpen(v)
          if (!v) setSelected(null)
        }}
      />
    </div>
  )
}
