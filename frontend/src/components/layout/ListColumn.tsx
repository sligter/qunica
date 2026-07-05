import { useState } from 'react'
import { Link, NavLink } from 'react-router-dom'
import { Plus, Search } from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

export interface ListColumnItem {
  id: string
  to: string
  name: string
  summary: string
  avatarClass: string
  avatarInitial: string
}

export interface ListColumnProps {
  title: string
  newTo: string
  newLabel: string
  searchPlaceholder: string
  isLoading: boolean
  loadError: boolean
  errorText: string
  emptyText: string
  items: ListColumnItem[]
  width?: number
  className?: string
}

/**
 * Generic entity list column (used inside the settings surface): header with
 * a "new" link, local search filter, and avatar/name/summary rows highlighted
 * by route match.
 */
export function ListColumn({
  title,
  newTo,
  newLabel,
  searchPlaceholder,
  isLoading,
  loadError,
  errorText,
  emptyText,
  items,
  width,
  className,
}: ListColumnProps) {
  const [query, setQuery] = useState('')
  const q = query.trim().toLowerCase()
  const filtered = q
    ? items.filter(
        (item) =>
          item.name.toLowerCase().includes(q) || item.summary.toLowerCase().includes(q),
      )
    : items

  return (
    <div
      className={cn(
        'flex h-full shrink-0 flex-col border-r border-border bg-background',
        width === undefined && 'w-72',
        className,
      )}
      style={width === undefined ? undefined : { width }}
    >
      <div className="flex h-14 shrink-0 items-center justify-between border-b border-border px-4">
        <h2 className="text-sm font-semibold">{title}</h2>
        <Button variant="ghost" size="icon" asChild>
          <Link to={newTo} aria-label={newLabel}>
            <Plus className="h-4 w-4" />
          </Link>
        </Button>
      </div>

      <div className="shrink-0 px-3 pt-2">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={searchPlaceholder}
            aria-label={searchPlaceholder}
            className="h-8 pl-8 text-xs"
          />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        {isLoading && <p className="px-4 text-xs text-muted-foreground">Loading…</p>}
        {loadError && <p className="px-4 text-xs text-destructive">{errorText}</p>}
        {!isLoading && !loadError && items.length === 0 && (
          <p className="px-4 text-xs text-muted-foreground">{emptyText}</p>
        )}
        {!isLoading && !loadError && items.length > 0 && filtered.length === 0 && (
          <p className="px-4 text-xs text-muted-foreground">No matches.</p>
        )}
        <ul className="space-y-0.5 px-2">
          {filtered.map((item) => (
            <li key={item.id}>
              <NavLink
                to={item.to}
                className={({ isActive }) =>
                  cn(
                    'flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                    isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                  )
                }
              >
                {({ isActive }) => (
                  <>
                    <Avatar className="h-9 w-9 shrink-0">
                      <AvatarFallback className={item.avatarClass}>
                        {item.avatarInitial}
                      </AvatarFallback>
                    </Avatar>
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span
                        className={cn(
                          'truncate text-sm',
                          isActive ? 'font-semibold' : 'font-medium',
                        )}
                      >
                        {item.name}
                      </span>
                      <p className="line-clamp-1 text-xs text-muted-foreground">
                        {item.summary}
                      </p>
                    </div>
                  </>
                )}
              </NavLink>
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}
