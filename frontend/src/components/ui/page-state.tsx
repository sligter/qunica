import type { ReactNode } from 'react'
import { AlertTriangle, Inbox, Loader2, type LucideIcon } from 'lucide-react'

import { cn } from '@/lib/utils'

type PageStateVariant = 'loading' | 'error' | 'empty'

const VARIANT_ICON: Record<PageStateVariant, LucideIcon> = {
  loading: Loader2,
  error: AlertTriangle,
  empty: Inbox,
}

interface PageStateProps {
  /** Drives the default icon and tone. */
  variant?: PageStateVariant
  /** The message. Kept short — one line. */
  title: ReactNode
  /** Optional second line with detail or a hint. */
  description?: ReactNode
  /** Overrides the variant's default icon. Pass `null` to drop the icon. */
  icon?: LucideIcon | null
  /** Optional call to action under the text (a Button, a Link). */
  action?: ReactNode
  /**
   * Render as a compact left-aligned block instead of filling and centering in
   * the available height. Use inside panels and columns.
   */
  inset?: boolean
  className?: string
}

/**
 * The single loading / error / empty surface. Every route and panel renders its
 * non-content states through this so a missing record, a failed fetch and an
 * empty list all look like the same product.
 *
 * Centered states own their pane, so the message is that pane's heading. Inset
 * states are a note beside content that already has one.
 */
export function PageState({
  variant = 'empty',
  title,
  description,
  icon,
  action,
  inset = false,
  className,
}: PageStateProps) {
  const Icon = icon === null ? null : (icon ?? VARIANT_ICON[variant])
  const isError = variant === 'error'
  const Title = inset ? 'p' : 'h2'

  return (
    <div
      role={isError ? 'alert' : undefined}
      aria-busy={variant === 'loading' || undefined}
      className={cn(
        inset
          ? 'flex items-start gap-3 px-6 py-8 text-left'
          : 'flex h-full min-h-0 w-full flex-1 flex-col items-center justify-center gap-3 bg-background p-8 text-center',
        className,
      )}
    >
      {Icon ? (
        <span
          className={cn(
            'flex shrink-0 items-center justify-center rounded-full',
            inset ? 'mt-0.5 h-8 w-8' : 'h-14 w-14',
            isError ? 'bg-destructive/10 text-destructive' : 'bg-muted text-muted-foreground',
          )}
        >
          <Icon
            className={cn(
              inset ? 'h-4 w-4' : 'h-6 w-6',
              variant === 'loading' && 'animate-spin',
            )}
          />
        </span>
      ) : null}
      <div className={cn('min-w-0', inset ? undefined : 'flex flex-col items-center gap-1.5')}>
        <Title
          className={cn(
            inset ? 'text-sm font-medium' : 'text-base font-medium',
            isError && 'text-destructive',
          )}
        >
          {title}
        </Title>
        {description ? (
          <p
            className={cn(
              'leading-relaxed text-muted-foreground',
              inset ? 'mt-1 text-xs' : 'max-w-sm text-sm',
            )}
          >
            {description}
          </p>
        ) : null}
        {action ? <div className={cn(inset ? 'mt-3' : 'mt-2')}>{action}</div> : null}
      </div>
    </div>
  )
}
