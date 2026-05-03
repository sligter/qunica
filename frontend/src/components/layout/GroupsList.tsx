import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { Plus } from 'lucide-react'

import { CreateGroupForm } from '@/components/groups/CreateGroupForm'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { useGroups } from '@/hooks/useGroups'
import { cn } from '@/lib/utils'

function avatarColor(seed: string): string {
  // Stable hue from the first chars of the group id/name. Limited to a few
  // pleasing pastels so the rail looks calm.
  const palette = [
    'bg-blue-500/90 text-white',
    'bg-emerald-500/90 text-white',
    'bg-amber-500/90 text-white',
    'bg-violet-500/90 text-white',
    'bg-rose-500/90 text-white',
    'bg-teal-500/90 text-white',
    'bg-indigo-500/90 text-white',
    'bg-orange-500/90 text-white',
  ]
  let h = 0
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0
  return palette[h % palette.length]!
}

function relativeTime(iso: string): string {
  const d = new Date(iso).getTime()
  const diff = Date.now() - d
  if (diff < 60_000) return 'now'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h`
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)}d`
  return new Date(iso).toLocaleDateString()
}

export function GroupsList() {
  const groups = useGroups()
  const { groupId: activeId } = useParams<{ groupId: string }>()
  const navigate = useNavigate()
  const [showForm, setShowForm] = useState(false)

  return (
    <div className="flex h-full w-72 shrink-0 flex-col border-r border-border bg-background">
      <div className="flex h-14 items-center justify-between border-b border-border px-4">
        <h2 className="text-sm font-semibold">Groups</h2>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => setShowForm((v) => !v)}
          aria-label="New group"
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>

      {showForm && (
        <div className="border-b border-border bg-card px-3 py-3">
          <CreateGroupForm
            onCreated={(id) => {
              setShowForm(false)
              void navigate(`/groups/${id}`)
            }}
          />
        </div>
      )}

      <div className="flex-1 overflow-y-auto py-2">
        {groups.isLoading && (
          <p className="px-4 text-xs text-muted-foreground">Loading…</p>
        )}
        {groups.error && (
          <p className="px-4 text-xs text-red-600">Failed to load groups.</p>
        )}
        {groups.data && groups.data.length === 0 && (
          <p className="px-4 text-xs text-muted-foreground">
            No groups yet. Click + to create one.
          </p>
        )}
        <ul className="space-y-0.5 px-2">
          {(groups.data ?? []).map((g) => {
            const isActive = g.id === activeId
            return (
              <li key={g.id}>
                <button
                  type="button"
                  onClick={() => void navigate(`/groups/${g.id}`)}
                  className={cn(
                    'flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                    isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                  )}
                >
                  <Avatar className="h-9 w-9 shrink-0">
                    <AvatarFallback className={avatarColor(g.id)}>
                      {g.name.slice(0, 1).toUpperCase()}
                    </AvatarFallback>
                  </Avatar>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <div className="flex items-baseline justify-between gap-2">
                      <span
                        className={cn(
                          'truncate text-sm',
                          isActive ? 'font-semibold' : 'font-medium',
                        )}
                      >
                        {g.name}
                      </span>
                      <span className="shrink-0 text-[10px] text-muted-foreground">
                        {relativeTime(g.created_at)}
                      </span>
                    </div>
                    <p className="line-clamp-1 text-xs text-muted-foreground">
                      {g.description || 'No description.'}
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
