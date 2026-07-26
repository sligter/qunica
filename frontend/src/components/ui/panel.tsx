import type { ReactNode } from 'react'
import type { LucideIcon } from 'lucide-react'

import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'

type PanelVariant = 'raised' | 'inset'

interface PanelProps {
  /** Panel title — the tier between a page heading and a section micro heading. */
  title: ReactNode
  /** Optional hint under the title. */
  description?: ReactNode
  /** Optional element on the right of the title row. */
  aside?: ReactNode
  /** Optional glyph before the title, for panels that name a subsystem. */
  icon?: LucideIcon
  /**
   * `raised` (default) is a standing surface — side panels, dashboard tiles.
   * `inset` is a group nested inside a form or another panel: tighter, flat.
   */
  variant?: PanelVariant
  /** Extra classes for the content wrapper under the header. */
  contentClassName?: string
  className?: string
  children?: ReactNode
}

const VARIANT_CLASS: Record<PanelVariant, string> = {
  raised: 'rounded-lg p-4',
  inset: 'rounded-md p-3 shadow-none',
}

const TITLE_CLASS: Record<PanelVariant, string> = {
  raised: 'text-sm font-semibold',
  inset: 'text-sm font-medium',
}

/** A raised panel is a top-level surface; an inset one is nested inside it. */
const TITLE_TAG: Record<PanelVariant, 'h2' | 'h3'> = {
  raised: 'h2',
  inset: 'h3',
}

/**
 * A titled card. `raised` sits above `Section` (a heading on the page
 * background) and below a page heading, so a column of panels reads as one
 * stack; `inset` gives a form's sub-groups that same header shape one step down
 * rather than each form inventing its own box.
 */
export function Panel({
  title,
  description,
  aside,
  icon: Icon,
  variant = 'raised',
  contentClassName,
  className,
  children,
}: PanelProps) {
  const Title = TITLE_TAG[variant]
  return (
    <Card asChild className={cn(VARIANT_CLASS[variant], className)}>
      <section>
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-start gap-2">
            {Icon ? (
              <Icon className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            ) : null}
            <div className="min-w-0">
              <Title className={cn('truncate', TITLE_CLASS[variant])}>{title}</Title>
              {description ? (
                <p className="mt-1 text-xs text-muted-foreground">{description}</p>
              ) : null}
            </div>
          </div>
          {aside ? <div className="shrink-0">{aside}</div> : null}
        </div>
        {children ? <div className={cn('mt-3', contentClassName)}>{children}</div> : null}
      </section>
    </Card>
  )
}
