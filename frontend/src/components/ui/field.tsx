import type { ReactNode } from 'react'
import { AlertCircle } from 'lucide-react'

import { Label } from '@/components/ui/label'
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

/**
 * One line of validation feedback.
 *
 * `role="alert"` so it is announced the moment it appears rather than waiting
 * for the next screen-reader pass, and the icon carries the tone for anyone who
 * cannot separate the hues — the message is never colour-only.
 */
export function FieldError({ id, className, children }: { id?: string; className?: string; children: ReactNode }) {
  return (
    <p
      id={id}
      role="alert"
      className={cn('flex items-start gap-1.5 text-xs text-destructive', className)}
    >
      <AlertCircle aria-hidden className="mt-px h-3.5 w-3.5 shrink-0" />
      <span className="min-w-0 break-words">{children}</span>
    </p>
  )
}

/** Attributes a control needs to be described by, and validated against, its field. */
export interface FormFieldControlProps {
  id: string
  'aria-describedby'?: string
  'aria-invalid'?: true
  'aria-required'?: true
}

interface FormFieldProps {
  /** Base id. Also seeds the description and error ids, so callers pass one string. */
  name: string
  label: ReactNode
  /** Marks the label with an asterisk and sets `aria-required` on the control. */
  required?: boolean
  /** Quiet hint under the control. Announced as part of the field's description. */
  description?: ReactNode
  /** Validation message. Renders a {@link FieldError} and flips the control invalid. */
  error?: ReactNode
  /** Small node on the label row's right — a counter, a secondary action. */
  aside?: ReactNode
  className?: string
  /** Receives the wired ids and aria attributes; spread them onto the control. */
  children: (props: FormFieldControlProps) => ReactNode
}

/**
 * Label + control + hint + error, wired together.
 *
 * Before this every form hand-rolled the same four elements and most of them
 * stopped after the label: the error was a loose `<p>` that nothing pointed at,
 * so a screen reader announced "required" with no way to reach the field that
 * was complaining. The control receives its ids through the render prop, which
 * keeps the wiring in one place without cloning elements into shapes they were
 * not written for.
 */
export function FormField({
  name,
  label,
  required = false,
  description,
  error,
  aside,
  className,
  children,
}: FormFieldProps) {
  const descriptionId = description ? `${name}-description` : undefined
  const errorId = error ? `${name}-error` : undefined
  // The error comes first: it is the thing you need to hear before the hint.
  const describedBy = [errorId, descriptionId].filter(Boolean).join(' ')

  return (
    <div className={cn('min-w-0 space-y-1.5', className)}>
      <div className="flex items-baseline justify-between gap-3">
        <Label htmlFor={name} className="min-w-0">
          {label}
          {required ? (
            <span aria-hidden className="ml-0.5 align-top text-destructive">
              *
            </span>
          ) : null}
        </Label>
        {aside ? <span className="shrink-0 text-2xs text-muted-foreground">{aside}</span> : null}
      </div>
      {children({
        id: name,
        'aria-describedby': describedBy || undefined,
        'aria-invalid': error ? true : undefined,
        'aria-required': required ? true : undefined,
      })}
      {description ? (
        <p id={descriptionId} className="text-2xs leading-relaxed text-muted-foreground">
          {description}
        </p>
      ) : null}
      {error ? <FieldError id={errorId}>{error}</FieldError> : null}
    </div>
  )
}
