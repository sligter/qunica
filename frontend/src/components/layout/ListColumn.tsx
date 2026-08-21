import { useMemo, useState } from 'react'
import { Link, NavLink } from 'react-router-dom'
import { Plus, Search, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { OverlayLink } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { SkeletonList } from '@/components/ui/skeleton'
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
  /** Optional per-item icon rendered in the row's avatar slot. */
  icon?: typeof Search
  width?: number
  className?: string
}

/**
 * Generic entity list column (the left pane of every resource-library area):
 * sticky header with a primary "new" action, a local search filter with a
 * live match count, avatar/name/summary rows highlighted by route match, and
 * skeleton / error / empty states so the column never collapses into a blank
 * strip while its query is in flight.
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
  icon: ItemIcon,
  width,
  className,
}: ListColumnProps) {
  const { t } = useTranslation('common')
  const [query, setQuery] = useState('')
  const q = query.trim().toLowerCase()
  const filtered = useMemo(
    () =>
      q
        ? items.filter(
            (item) =>
              item.name.toLowerCase().includes(q) ||
              item.summary.toLowerCase().includes(q),
          )
        : items,
    [items, q],
  )

  return (
    <div
      className={cn(
        'flex h-full shrink-0 flex-col border-r border-border bg-card/40',
        width === undefined && 'w-72',
        className,
      )}
      style={width === undefined ? undefined : { width }}
    >
      {/* Header: area title left, primary create action right. */}
      <div className="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-border px-3">
        <h2 className="truncate text-sm font-semibold tracking-tight">{title}</h2>
        <Button size="sm" variant="default" className="h-7 gap-1 px-2.5 text-xs shadow-xs" asChild>
          <Link to={newTo} aria-label={newLabel}>
            <Plus className="h-3.5 w-3.5" />
            {!width || width >= 56 ? (
              <span className="hidden lg:inline">{newLabel}</span>
            ) : null}
          </Link>
        </Button>
      </div>

      {/* Search filter with a clear button and a reserved-height match count so
          results appearing never shift the list downward. */}
      <div className="shrink-0 px-3 pb-2 pt-2.5">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={searchPlaceholder}
            aria-label={searchPlaceholder}
            className="h-8 rounded-lg bg-background pl-8 pr-8 text-xs"
          />
          {query ? (
            <button
              type="button"
              aria-label={t('state.clearSearch')}
              onClick={() => setQuery('')}
              className="absolute right-1.5 top-1/2 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <X className="h-3 w-3" />
            </button>
          ) : null}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-2">
        {isLoading ? (
          <SkeletonList rows={7} />
        ) : loadError ? (
          <PageState inset variant="error" icon={null} title={errorText} />
        ) : items.length === 0 ? (
          <PageState
            inset
            icon={ItemIcon}
            title={emptyText}
            description={t('state.emptyHint')}
            action={
              <Button size="sm" variant="outline" asChild>
                <OverlayLink to={newTo}>
                  <Plus className="h-3.5 w-3.5" />
                  {newLabel}
                </OverlayLink>
              </Button>
            }
          />
        ) : filtered.length === 0 ? (
          <PageState inset icon={Search} title={t('state.noMatches')} />
        ) : (
          <ul className="space-y-0.5" aria-label={title}>
            {filtered.map((item) => (
              <li key={item.id}>
                <NavLink
                  to={item.to}
                  title={`${item.name} — ${item.summary}`}
                  className={({ isActive }) =>
                    cn(
                      'group flex w-full items-start gap-2.5 rounded-lg px-2.5 py-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                      isActive
                        ? 'bg-primary/10'
                        : 'hover:bg-card-hover',
                    )
                  }
                >
                  {({ isActive }) => (
                    <>
                      <span
                        aria-hidden
                        className={cn(
                          'mt-0.5 flex h-8 w-8 shrink-0 select-none items-center justify-center rounded-full text-xs font-semibold',
                          item.avatarClass,
                        )}
                      >
                        {ItemIcon ? (
                          <ItemIcon className="h-4 w-4" />
                        ) : (
                          item.avatarInitial
                        )}
                      </span>
                      <div className="flex min-w-0 flex-1 flex-col gap-0.5 pt-0.5">
                        <span
                          className={cn(
                            'truncate text-sm leading-tight',
                            isActive ? 'font-semibold text-primary' : 'font-medium',
                          )}
                        >
                          {item.name}
                        </span>
                        <p className="truncate text-xs leading-tight text-muted-foreground group-hover:text-foreground/70">
                          {item.summary}
                        </p>
                      </div>
                    </>
                  )}
                </NavLink>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Sticky footer count — a quiet "how many do I have" without counting
          rows by eye. Doubles as the live region for filter results. */}
      {!isLoading && !loadError && items.length > 0 ? (
        <div
          aria-live="polite"
          className="shrink-0 border-t border-border px-3 py-1.5 text-2xs text-muted-foreground tabular-nums"
        >
          {q
            ? filtered.length > 0
              ? t('state.matchCount', { count: filtered.length })
              : t('state.noMatches')
            : t('state.totalCount', { count: items.length })}
        </div>
      ) : null}
    </div>
  )
}
