import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'
import { useDocumentTitle } from '@/components/ui/section'

/**
 * How wide the page's own content is allowed to run.
 *
 * `form` caps it. On a 2560px monitor an uncapped settings row puts its label
 * at the far left and its control at the far right with a metre of nothing
 * between them, and the eye loses the pairing.
 *
 * `wide` opts out, for the surfaces that genuinely use the room: log tables,
 * usage charts, and the group members master/detail grid.
 */
export type DetailMeasure = 'form' | 'wide'

const MEASURE_CLASS: Record<DetailMeasure, string> = {
  form: 'mx-auto max-w-[1120px]',
  wide: '',
}

interface DetailShellProps {
  /** Main heading, rendered in serif. */
  title: ReactNode
  /** Optional line under the title: description text, badges, etc. */
  subtitle?: ReactNode
  /** Right-aligned header actions (Edit/Delete/Cancel button group). */
  actions?: ReactNode
  /** Optional element before the title (e.g. a back button). */
  leading?: ReactNode
  /** Content width. Defaults to the capped reading measure. */
  measure?: DetailMeasure
  /** Extra classes for the content wrapper (e.g. an intentional max width). */
  contentClassName?: string
  children: ReactNode
}

/**
 * Shared shell for second-level pages (detail / create / settings / manage).
 * Header with title left and actions right; content below on the same measure,
 * so form and section edges stay aligned with those header actions.
 *
 * The measure wraps the header's *contents*, not the header itself: the rule
 * under it is the pane's own chrome and runs edge to edge, while the title and
 * the action buttons land on the same vertical lines as the sections below.
 * That alignment is what `SettingsSection` leans on when it fills its page
 * rather than capping itself — see `ui/settings-row.tsx`.
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
  measure = 'form',
  contentClassName,
  children,
}: DetailShellProps) {
  const measureClass = MEASURE_CLASS[measure]
  // String titles become the tab title; richer nodes (badges etc.) opt out.
  useDocumentTitle(typeof title === 'string' ? title : null)
  return (
    <div className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-background">
      <header className="flex min-h-14 shrink-0 items-center border-b border-border bg-background px-6 py-2.5">
        <div
          className={cn(
            'flex w-full min-w-0 items-center justify-between gap-4',
            measureClass,
          )}
        >
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
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div className={cn('w-full px-6 py-5 pb-8', measureClass, contentClassName)}>
          {children}
        </div>
      </div>
    </div>
  )
}
