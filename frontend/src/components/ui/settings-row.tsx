import type { ReactNode } from 'react'

import { SectionHeading } from '@/components/ui/section'
import { cn } from '@/lib/utils'

interface SettingsSectionProps {
  /** Uppercase micro heading for the section. */
  title: ReactNode
  /** Optional one-line section description under the heading. */
  description?: ReactNode
  /** Optional element on the right of the section heading (badge, Save button). */
  aside?: ReactNode
  className?: string
  children: ReactNode
}

/**
 * Claude-settings-style section: uppercase micro heading, then rows separated
 * by dividers.
 *
 * The section fills its page's content width rather than capping itself. A
 * narrower cap here would end the section's rules short of the page's own right
 * edge, leaving a band of dead space beside every setting; the page owns the
 * measure, the section just fills it. Rows stay readable because inline controls
 * share one right-flushed column (see {@link SettingsRow}).
 */
export function SettingsSection({
  title,
  description,
  aside,
  className,
  children,
}: SettingsSectionProps) {
  return (
    <section className={cn('w-full', className)}>
      <SectionHeading title={title} description={description} aside={aside} />
      <div className="divide-y divide-border">{children}</div>
    </section>
  )
}

/**
 * Width of the inline control column. Sized to the widest control any settings
 * screen puts in it (a segmented theme/language switch); narrower controls sit
 * flush right inside it. Below `sm` the row stacks and the column goes full
 * width, so a narrow window never squeezes a label against its control.
 */
const CONTROL_COLUMN = 'w-full sm:w-72'

/**
 * Reading measure for a row's description. The row itself spans the page so its
 * control lands on the page's right edge, but a sentence stretched across a wide
 * pane is tiring to read — so the prose wraps early while the layout does not.
 */
const LABEL_MEASURE = 'max-w-prose'

interface SettingsRowProps {
  /** Row label. Pass `htmlFor` to bind it to the control. */
  label: ReactNode
  /** One-line description under the label. */
  description?: ReactNode
  /** Associates the label with a control id via <label htmlFor>. */
  htmlFor?: string
  /**
   * Stack the control under the label instead of placing it on the right.
   * Use for wide controls (text inputs, textareas, selects with long values).
   */
  stacked?: boolean
  className?: string
  /** The control: switch, select, button group, input, ... */
  children?: ReactNode
}

/**
 * One setting per row: label + description on the left, control on the right.
 * With `stacked`, the control renders full-width under the label.
 *
 * Inline controls share one fixed-width column and are flushed right; stacked
 * controls fill the section. Both therefore end on the same vertical line, so a
 * switch, a number box and a select read as one column instead of three ragged
 * ones. Callers pass controls without their own widths — a number input keeps a
 * narrow `w-24`, everything else takes `w-full` and the column sizes it.
 */
export function SettingsRow({
  label,
  description,
  htmlFor,
  stacked = false,
  className,
  children,
}: SettingsRowProps) {
  const labelBlock = (
    <div className={cn('min-w-0', stacked ? undefined : 'flex-1')}>
      {htmlFor ? (
        <label htmlFor={htmlFor} className="text-sm font-medium leading-none">
          {label}
        </label>
      ) : (
        <p className="text-sm font-medium leading-none">{label}</p>
      )}
      {description ? (
        <p className={cn('mt-0.5 text-xs text-muted-foreground', LABEL_MEASURE)}>
          {description}
        </p>
      ) : null}
    </div>
  )

  if (stacked) {
    return (
      <div
        data-slot="settings-row"
        data-stacked=""
        className={cn('space-y-1.5 py-2.5', className)}
      >
        {labelBlock}
        {children}
      </div>
    )
  }

  return (
    <div
      data-slot="settings-row"
      className={cn(
        'flex flex-col gap-2 py-2.5 sm:flex-row sm:items-center sm:justify-between sm:gap-6',
        className,
      )}
    >
      {labelBlock}
      {children ? (
        <div
          data-slot="settings-control"
          className={cn(
            'flex items-center gap-2 sm:shrink-0 sm:justify-end',
            CONTROL_COLUMN,
          )}
        >
          {children}
        </div>
      ) : null}
    </div>
  )
}
