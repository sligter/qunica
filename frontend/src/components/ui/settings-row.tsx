import type { ReactNode } from 'react'

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
      <div className="flex items-end justify-between gap-4 border-b border-border pb-2">
        <div className="min-w-0">
          <h2 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            {title}
          </h2>
          {description ? (
            <p className="mt-1 text-xs text-muted-foreground">{description}</p>
          ) : null}
        </div>
        {aside ? <div className="shrink-0">{aside}</div> : null}
      </div>
      <div className="divide-y divide-border">{children}</div>
    </section>
  )
}

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
        <p className="mt-1 text-xs text-muted-foreground">{description}</p>
      ) : null}
    </div>
  )

  if (stacked) {
    return (
      <div className={cn('space-y-2 py-4', className)}>
        {labelBlock}
        {children}
      </div>
    )
  }

  return (
    <div className={cn('flex items-center justify-between gap-6 py-4', className)}>
      {labelBlock}
      {children ? <div className="flex shrink-0 items-center gap-2">{children}</div> : null}
    </div>
  )
}
