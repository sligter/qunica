import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

interface SectionHeadingProps {
  /** Uppercase micro heading text. */
  title: ReactNode
  /** Optional one-line description under the heading. */
  description?: ReactNode
  /** Optional element on the right of the heading (badge, Save button). */
  aside?: ReactNode
  /** Heading level. Defaults to h2; use h3 for headings nested inside a section. */
  as?: 'h2' | 'h3'
  className?: string
}

/**
 * The one micro heading used across the app: uppercase, letter-spaced, muted,
 * with a hairline rule under it. Every section — settings rows, detail fields,
 * read-only panels — opens with this so the pages share one editorial rhythm.
 */
export function SectionHeading({
  title,
  description,
  aside,
  as: Heading = 'h2',
  className,
}: SectionHeadingProps) {
  return (
    <div
      className={cn('flex items-end justify-between gap-4 border-b border-border pb-2', className)}
    >
      <div className="min-w-0">
        <Heading className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {title}
        </Heading>
        {description ? (
          // Wraps at a reading measure even though the heading rule runs the
          // full width of the page.
          <p className="mt-1 max-w-prose text-xs text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {aside ? <div className="shrink-0">{aside}</div> : null}
    </div>
  )
}

interface SectionProps extends SectionHeadingProps {
  /** Extra classes for the content wrapper under the heading. */
  contentClassName?: string
  children: ReactNode
}

/**
 * A titled block of read-only content: {@link SectionHeading} followed by the
 * content. Use for detail pages; use `SettingsSection` when the body is a list
 * of label/control rows that want dividers.
 */
export function Section({
  title,
  description,
  aside,
  as,
  className,
  contentClassName,
  children,
}: SectionProps) {
  return (
    <section className={cn('w-full', className)}>
      <SectionHeading title={title} description={description} aside={aside} as={as} />
      <div className={cn('pt-3', contentClassName)}>{children}</div>
    </section>
  )
}
