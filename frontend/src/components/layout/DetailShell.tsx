import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

interface DetailShellProps {
  /** Main heading, rendered in serif. */
  title: ReactNode
  /** Optional line under the title: description text, badges, etc. */
  subtitle?: ReactNode
  /** Right-aligned header actions (Edit/Delete/Cancel button group). */
  actions?: ReactNode
  /** Optional element before the title (e.g. a back button). */
  leading?: ReactNode
  /** Extra classes for the content wrapper (e.g. an intentional max width). */
  contentClassName?: string
  children: ReactNode
}

/**
 * Shared shell for second-level pages (detail / create / settings / manage).
 * Header with title left and actions right; full-width content keeps form and
 * section edges aligned with those header actions.
 *
 * The header is deliberately the same height and type size as the ones in
 * `SettingsLayout` and `EntityLayout`: those three plus the group manage page
 * are the app's second-level surfaces, and a title that grows a step between
 * them is what made them read as four unrelated screens.
 */
export function DetailShell({
  title,
  subtitle,
  actions,
  leading,
  contentClassName,
  children,
}: DetailShellProps) {
  return (
    <div className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-background">
      <header className="flex min-h-14 shrink-0 items-center justify-between gap-4 border-b border-border bg-background px-6 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          {leading}
          <div className="min-w-0">
            <h1 className="truncate font-serif text-base font-semibold tracking-tight">
              {title}
            </h1>
            {subtitle ? (
              <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-2 text-xs text-muted-foreground">
                {subtitle}
              </div>
            ) : null}
          </div>
        </div>
        {actions ? (
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        ) : null}
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div className={cn('w-full px-6 py-5 pb-8', contentClassName)}>
          {children}
        </div>
      </div>
    </div>
  )
}
