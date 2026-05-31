import { useNavigate, useParams } from 'react-router-dom'
import { Plus } from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { useAgents } from '@/hooks/useAgents'
import { cn } from '@/lib/utils'

export function AgentsList() {
  const agents = useAgents()
  const { agentId: activeId } = useParams<{ agentId: string }>()
  const navigate = useNavigate()
  // /agents (no id) is the "create new" pane; treat it as the active row hint.
  const isCreateView = !activeId

  return (
    <div className="flex h-full w-72 shrink-0 flex-col border-r border-border bg-background">
      <div className="flex h-14 items-center justify-between border-b border-border px-4">
        <h2 className="text-sm font-semibold">Agents</h2>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void navigate('/agents')}
          aria-label="New agent"
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        {agents.isLoading && (
          <p className="px-4 text-xs text-muted-foreground">Loading…</p>
        )}
        {agents.error && (
          <p className="px-4 text-xs text-red-600">Failed to load agents.</p>
        )}
        {agents.data && agents.data.length === 0 && (
          <p className="px-4 text-xs text-muted-foreground">
            No agents yet. Click + to create one.
          </p>
        )}

        {isCreateView && agents.data && agents.data.length > 0 && (
          <p className="mb-2 px-3 text-[10px] uppercase tracking-wider text-muted-foreground">
            New agent — fill the form on the right.
          </p>
        )}

        <ul className="space-y-0.5 px-2">
          {(agents.data ?? []).map((a) => {
            const isActive = a.id === activeId
            return (
              <li key={a.id}>
                <button
                  type="button"
                  onClick={() => void navigate(`/agents/${a.id}`)}
                  className={cn(
                    'flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                    isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                  )}
                >
                  <Avatar className="h-9 w-9 shrink-0">
                    <AvatarFallback className="bg-emerald-500/90 text-white">
                      {a.name.slice(0, 1).toUpperCase()}
                    </AvatarFallback>
                  </Avatar>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span
                      className={cn(
                        'truncate text-sm',
                        isActive ? 'font-semibold' : 'font-medium',
                      )}
                    >
                      {a.name}
                    </span>
                    <p className="line-clamp-1 text-xs text-muted-foreground">
                      {a.runtime_kind === 'external_cli'
                        ? 'External CLI'
                        : a.description || a.system_prompt}
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
