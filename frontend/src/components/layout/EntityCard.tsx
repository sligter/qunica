import type { ComponentType, ReactNode } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { ArrowRight, Pencil } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/lib/utils'

export interface EntityCardStat {
  /** Stable key so React does not need positional reconciliation. */
  key: string
  icon?: ComponentType<{ className?: string }>
  content: ReactNode
}

interface EntityCardProps {
  to: string
  /**
   * Where the hover edit affordance routes. Omit for entities whose only
   * mutation surface is the card's own destination (e.g. workspace detail
   * edits inline).
   */
  editTo?: string
  title: string
  description?: ReactNode
  avatarInitial?: string
  avatarClass?: string
  /** Optional glyph instead of an initial (MCP server, provider). */
  avatarIcon?: ReactNode
  statusLabel?: string
  statusActive?: boolean
  /** Typed badge next to the title (transport, kind, source). */
  metaBadge?: { label: string; className?: string }
  stats?: EntityCardStat[]
}

/**
 * One gallery card shared by every resource-library index page.
 *
 * The whole card links to the detail view; a quiet pencil in the top-right on
 * hover deep-links straight into the edit form (`?edit=1`), so editing is one
 * click from anywhere instead of open-then-find-the-button.
 */
export function EntityCard({
  to,
  editTo,
  title,
  description,
  avatarInitial,
  avatarClass,
  avatarIcon,
  statusLabel,
  statusActive,
  metaBadge,
  stats,
}: EntityCardProps) {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  return (
    <div
      className={cn(
        'group relative flex flex-col justify-between rounded-xl border border-border/80 bg-card p-3',
        'transition-all duration-200 hover:-translate-y-0.5 hover:border-primary/40 hover:shadow-md',
      )}
    >
      {/* Stretched link: the whole surface navigates. Everything else in the
          card stays unpositioned so its text cannot win the hit test — the
          classic mistake is wrapping content in `relative`, which stacks it
          above the link and makes clicks on words do nothing. Only controls
          that need their own click (the pencil) opt into positioning, which
          paints them back above the link by DOM order. */}
      <Link
        to={to}
        aria-label={title}
        className="absolute inset-0 rounded-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      <div>
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2.5">
            <span
              aria-hidden
              className={cn(
                'flex h-9 w-9 shrink-0 select-none items-center justify-center rounded-lg text-sm font-semibold shadow-xs transition-transform group-hover:scale-105',
                !avatarIcon && 'uppercase',
                avatarClass ?? 'bg-primary/10 text-primary',
              )}
            >
              {avatarIcon ?? avatarInitial}
            </span>
            <div className="min-w-0">
              <h3 className="truncate text-sm font-semibold text-foreground transition-colors group-hover:text-primary">
                {title}
              </h3>
              {metaBadge ? (
                <span
                  className={cn(
                    'mt-0.5 inline-block rounded-md border px-1.5 py-0.5 text-2xs font-medium uppercase leading-none',
                    metaBadge.className,
                  )}
                >
                  {metaBadge.label}
                </span>
              ) : null}
            </div>
          </div>

          <span className="flex shrink-0 items-center gap-1.5">
            {statusLabel ? (
              <>
                <span
                  aria-hidden
                  className={cn(
                    'h-2 w-2 rounded-full',
                    statusActive ? 'bg-success' : 'bg-muted-foreground/40',
                  )}
                />
                <span className="text-2xs font-medium text-muted-foreground">{statusLabel}</span>
              </>
            ) : null}
            {editTo ? (
              <button
                type="button"
                onClick={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                  navigate(editTo)
                }}
                // `p-1.5` rather than `p-1`: with the 14px glyph that is a 26px
                // target, where the old 22px one sat under the smallest size a
                // pointer or thumb lands on reliably.
                //
                // `pointer-coarse:opacity-100` matters more than it looks: the
                // pencil used to be reachable only through hover, which a
                // touchscreen never produces — on a tablet there was no way to
                // edit from the gallery at all.
                className="relative rounded-md p-1.5 text-muted-foreground opacity-0 transition-[opacity,color,background-color] hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100 pointer-coarse:opacity-100"
                aria-label={t('actions.edit')}
                title={t('actions.edit')}
              >
                <Pencil className="h-3.5 w-3.5" />
              </button>
            ) : null}
          </span>
        </div>

        {description ? (
          <p className="mt-2 line-clamp-2 text-xs leading-relaxed text-muted-foreground [&_code]:rounded [&_code]:bg-muted [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-2xs">
            {description}
          </p>
        ) : null}
      </div>

      <div className="mt-3 flex items-center justify-between border-t border-border/50 pt-2.5 text-xs text-muted-foreground">
        <div className="flex items-center gap-1.5">
          {(stats ?? []).map((stat) => (
            <span
              key={stat.key}
              className="inline-flex items-center gap-1 rounded-md bg-muted px-1.5 py-0.5 text-2xs font-medium text-foreground/80"
            >
              {stat.icon ? <stat.icon className="h-3 w-3 text-primary" /> : null}
              {stat.content}
            </span>
          ))}
        </div>
        <span className="flex items-center gap-1 text-2xs font-medium text-primary opacity-0 transition-opacity group-hover:opacity-100">
          {t('actions.view')}
          <ArrowRight className="h-3 w-3" />
        </span>
      </div>
    </div>
  )
}
