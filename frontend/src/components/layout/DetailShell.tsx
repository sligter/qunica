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
  /** Extra classes for the content wrapper (e.g. a wider max width). */
  contentClassName?: string
  children: ReactNode
}

/**
 * Shared shell for second-level pages (detail / create / settings / manage).
 * Sticky header with title left and actions right; full-width, left-aligned
 * content with a generous max width to keep lines readable on very wide
 * screens.
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
      <header className="z-10 flex shrink-0 items-center justify-between gap-4 border-b border-border bg-background/95 px-8 py-4 backdrop-blur">
        <div className="flex min-w-0 items-center gap-3">
          {leading}
          <div className="min-w-0">
            <h1 className="truncate font-serif text-xl font-semibold tracking-tight">
              {title}
            </h1>
            {subtitle ? (
              <div className="mt-1 flex min-w-0 flex-wrap items-center gap-2 text-sm text-muted-foreground">
                {subtitle}
              </div>
            ) : null}
          </div>
        </div>
        {actions ? (
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        ) : null}
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className={cn('w-full max-w-5xl px-8 py-6 pb-10', contentClassName)}>
          {children}
        </div>
      </div>
    </div>
  )
}
