import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

interface FieldGridProps {
  /** Widest column count, reached at xl. Narrower breakpoints step down. */
  columns?: 2 | 3 | 4
  className?: string
  children: ReactNode
}

const COLUMN_CLASS: Record<2 | 3 | 4, string> = {
  2: 'sm:grid-cols-2',
  3: 'sm:grid-cols-2 xl:grid-cols-3',
  4: 'sm:grid-cols-2 xl:grid-cols-4',
}

/** Responsive grid of read-only {@link Field}s with the shared column rhythm. */
export function FieldGrid({ columns = 3, className, children }: FieldGridProps) {
  return (
    <div className={cn('grid grid-cols-1 gap-x-8 gap-y-4', COLUMN_CLASS[columns], className)}>
      {children}
    </div>
  )
}

interface FieldProps {
  /** Field name, rendered as a muted micro label above the value. */
  label: ReactNode
  /** Text value. Ignored when `children` is provided. */
  value?: ReactNode
  /** Renders the value in the mono face — for keys, ids, paths. */
  mono?: boolean
  className?: string
  /** Custom value content (a Badge, a link, a list). Takes precedence over `value`. */
  children?: ReactNode
}

/**
 * One read-only label/value pair. The label matches the section micro heading
 * so a field grid and a section stack read as one hierarchy.
 */
export function Field({ label, value, mono, className, children }: FieldProps) {
  return (
    <div className={cn('min-w-0', className)}>
      <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </h3>
      {children ?? (
        <p className={cn('mt-1 break-words text-sm', mono && 'font-mono text-xs')}>{value}</p>
      )}
    </div>
  )
}
