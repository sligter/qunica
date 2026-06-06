import { useNavigate, useParams } from 'react-router-dom'
import { Plus } from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { useProviders } from '@/hooks/useProviders'
import { cn } from '@/lib/utils'
import type { ProviderKind } from '@/types/api'

function kindColor(kind: ProviderKind): string {
  if (kind === 'anthropic') return 'bg-orange-500/90 text-white'
  if (kind === 'anthropic-compatible') return 'bg-amber-500/90 text-white'
  if (kind === 'gemini') return 'bg-blue-500/90 text-white'
  return 'bg-violet-500/90 text-white'
}

function kindInitial(kind: ProviderKind, name: string): string {
  if (kind === 'anthropic') return 'A'
  if (kind === 'anthropic-compatible') return 'C'
  if (kind === 'gemini') return 'G'
  return name.slice(0, 1).toUpperCase()
}

export function ProvidersList() {
  const providers = useProviders()
  const { providerId: activeId } = useParams<{ providerId: string }>()
  const navigate = useNavigate()
  const isCreateView = !activeId

  return (
    <div className="flex h-full w-72 shrink-0 flex-col border-r border-border bg-background">
      <div className="flex h-14 items-center justify-between border-b border-border px-4">
        <h2 className="text-sm font-semibold">Providers</h2>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void navigate('/providers')}
          aria-label="New provider"
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        {providers.isLoading && (
          <p className="px-4 text-xs text-muted-foreground">Loading...</p>
        )}
        {providers.error && (
          <p className="px-4 text-xs text-red-600">Failed to load providers.</p>
        )}
        {providers.data && providers.data.length === 0 && (
          <p className="px-4 text-xs text-muted-foreground">
            No providers yet. Click + to register OpenAI, Anthropic-compatible, or Gemini.
          </p>
        )}

        {isCreateView && providers.data && providers.data.length > 0 && (
          <p className="mb-2 px-3 text-[10px] uppercase text-muted-foreground">
            New provider - fill the form on the right.
          </p>
        )}

        <ul className="space-y-0.5 px-2">
          {(providers.data ?? []).map((p) => {
            const isActive = p.id === activeId
            return (
              <li key={p.id}>
                <button
                  type="button"
                  onClick={() => void navigate(`/providers/${p.id}`)}
                  className={cn(
                    'flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                    isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                  )}
                >
                  <Avatar className="h-9 w-9 shrink-0">
                    <AvatarFallback className={kindColor(p.kind)}>
                      {kindInitial(p.kind, p.name)}
                    </AvatarFallback>
                  </Avatar>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span
                      className={cn(
                        'truncate text-sm',
                        isActive ? 'font-semibold' : 'font-medium',
                      )}
                    >
                      {p.name}
                    </span>
                    <p className="line-clamp-1 text-xs text-muted-foreground">
                      {p.kind} - {p.default_model}
                    </p>
                  </div>
                </button>
              </li>
            )
          })}
        </ul>
      </div>
    </div>
  )
}
