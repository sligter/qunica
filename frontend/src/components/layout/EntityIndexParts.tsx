import type { ComponentType, ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { SearchX } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

/**
 * Pieces shared by every resource-library index page.
 *
 * Each of the five pages had its own copy of the metric row and the empty
 * panel, pasted and then edited one card at a time — which is how the agents
 * grid ended up on a different column count from the rest and how amber-500
 * got in beside the theme tokens. They are one component each now, so a change
 * to the metric card lands on all five at once.
 */

type MetricTone = 'default' | 'primary' | 'success' | 'info' | 'warning'

/** Every tone resolves to a theme token — no literal palette colours. */
const TONE_VALUE: Record<MetricTone, string> = {
  default: 'text-foreground',
  primary: 'text-primary',
  success: 'text-success',
  info: 'text-info',
  warning: 'text-warning-foreground',
}

const TONE_ICON: Record<MetricTone, string> = {
  default: 'text-muted-foreground',
  primary: 'text-primary/70',
  success: 'text-success/70',
  info: 'text-info/70',
  warning: 'text-warning-foreground/70',
}

/**
 * The metric strip's grid.
 *
 * Four columns from `lg` up and two below it, so a row of three metrics sits
 * left-aligned with one empty cell rather than dropping a single orphan card
 * onto a second line — which is what the old two-column-then-three step did.
 */
export function MetricRow({
  className,
  children,
}: {
  className?: string
  children: ReactNode
}) {
  return (
    <div className={cn('grid grid-cols-2 gap-3 lg:grid-cols-4', className)}>{children}</div>
  )
}

interface MetricCardProps {
  label: ReactNode
  value: ReactNode
  /** Glyph in the top-right; pass a dot or a count badge for status metrics. */
  icon?: ComponentType<{ className?: string }>
  /** Right-hand marker instead of a glyph — used by the "active / enabled" dots. */
  marker?: ReactNode
  tone?: MetricTone
}

/** One figure with its name above it: the summary strip above a gallery grid. */
export function MetricCard({ label, value, icon: Icon, marker, tone = 'default' }: MetricCardProps) {
  return (
    <div className="rounded-xl border border-border/80 bg-card/60 px-3.5 py-3 shadow-xs">
      <div className="flex items-start justify-between gap-2">
        <span className="min-w-0 truncate text-2xs font-medium text-muted-foreground">{label}</span>
        {marker ?? (Icon ? <Icon aria-hidden className={cn('h-3.5 w-3.5 shrink-0', TONE_ICON[tone])} /> : null)}
      </div>
      <p className={cn('mt-1 text-xl font-semibold tabular-nums tracking-tight', TONE_VALUE[tone])}>
        {value}
      </p>
    </div>
  )
}

interface EntityEmptyStateProps {
  icon: ComponentType<{ className?: string }>
  title: ReactNode
  description?: ReactNode
  actionLabel?: ReactNode
  actionTo?: string
  iconNode?: ReactNode
  className?: string
}

/**
 * The "nothing here yet" panel. Dashed so it reads as a place a card will go
 * rather than as a card that failed to render, with the create action inside it
 * because that is the only thing to do from here.
 */
export function EntityEmptyState({
  icon: Icon,
  title,
  description,
  actionLabel,
  actionTo,
  className,
}: EntityEmptyStateProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 bg-card/30 p-8 text-center sm:p-12',
        className,
      )}
    >
      <span
        aria-hidden
        className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary"
      >
        <Icon className="h-6 w-6" />
      </span>
      <h3 className="text-base font-semibold">{title}</h3>
      {description ? (
        <p className="mt-1 max-w-sm text-sm text-muted-foreground">{description}</p>
      ) : null}
      {actionLabel && actionTo ? (
        <Button className="mt-6 gap-2" asChild>
          {/* Router Link, not <a>: this is an in-app route, and a plain anchor
              would reload the whole shell just to open the create page. */}
          <Link to={actionTo}>{actionLabel}</Link>
        </Button>
      ) : null}
    </div>
  )
}

/**
 * "Your search matched nothing" — a distinct case from having no records at
 * all, so it gets its own quiet line instead of the full create panel.
 */
export function NoMatchesState({ message }: { message: ReactNode }) {
  return (
    <div className="col-span-full flex flex-col items-center gap-2 rounded-xl border border-border/60 bg-card/40 py-10 text-center">
      <SearchX aria-hidden className="h-5 w-5 text-muted-foreground" />
      <p className="text-sm text-muted-foreground">{message}</p>
    </div>
  )
}

/**
 * A failed index query.
 *
 * Distinct from the empty state on purpose: "we could not reach the server" and
 * "you have none of these" are different situations, and collapsing them into
 * one panel is how a network blip used to read as an empty library.
 */
export function IndexErrorState({
  title,
  detail,
  onRetry,
  retryLabel,
}: {
  title: ReactNode
  detail?: ReactNode
  onRetry?: () => void
  retryLabel?: ReactNode
}) {
  return (
    <PageState
      variant="error"
      title={title}
      description={detail}
      className="min-h-[60vh]"
      action={
        onRetry ? (
          <Button size="sm" variant="outline" onClick={onRetry}>
            {retryLabel}
          </Button>
        ) : null
      }
    />
  )
}

/**
 * Loading placeholder shaped like the gallery it stands in for: the same metric
 * strip and the same card grid, so arriving data shifts nothing.
 */
export function EntityIndexSkeleton({ cards = 6 }: { cards?: number }) {
  return (
    <div className="space-y-6" aria-hidden>
      <MetricRow>
        {Array.from({ length: 4 }, (_, index) => (
          <div key={index} className="rounded-xl border border-border/80 bg-card/60 px-3.5 py-3 shadow-xs">
            <div className="flex items-start justify-between gap-2">
              <Skeleton className="h-2.5 w-16" />
              <Skeleton className="h-3.5 w-3.5 rounded-sm" />
            </div>
            <Skeleton className="mt-1 h-6 w-12" />
          </div>
        ))}
      </MetricRow>
      {/* Mirrors `EntityCard` exactly — same padding, avatar, rhythm and
          footer — so the swap to real content shifts nothing. */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
        {Array.from({ length: cards }, (_, index) => (
          <div key={index} className="rounded-xl border border-border/80 bg-card p-3">
            <div className="flex items-start gap-2.5">
              <Skeleton className="h-9 w-9 rounded-lg" />
              <div className="min-w-0 flex-1 space-y-2 pt-1">
                <Skeleton className="h-3 w-2/3" />
                <Skeleton className="h-2.5 w-16" />
              </div>
            </div>
            <Skeleton className="mt-2 h-2.5 w-full opacity-60" />
            <Skeleton className="mt-1.5 h-2.5 w-5/6 opacity-60" />
            <div className="mt-3 flex items-center justify-between border-t border-border/50 pt-2.5">
              <Skeleton className="h-4 w-14 rounded-md" />
              <Skeleton className="h-3 w-12 opacity-60" />
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
