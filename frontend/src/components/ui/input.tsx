import * as React from 'react'

import { cn } from '@/lib/utils'

export type InputProps = React.InputHTMLAttributes<HTMLInputElement>

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          'flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm transition-colors',
          'placeholder:text-muted-foreground',
          // One focus vocabulary, shared with every other control: a 2px ring
          // offset off the border so it reads as a halo, not as a thicker edge.
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background',
          // Disabled reads as inert rather than merely faded: the surface drops
          // back to the muted tone so the field stops inviting typing.
          'disabled:cursor-not-allowed disabled:border-transparent disabled:bg-muted/60 disabled:text-muted-foreground disabled:shadow-none',
          // Invalid lives on the control too, not only in the message below it
          // — a red line of text is invisible to anyone who tabbed straight to
          // the next field.
          'aria-[invalid=true]:border-destructive aria-[invalid=true]:focus-visible:ring-destructive',
          className,
        )}
        ref={ref}
        {...props}
      />
    )
  },
)
Input.displayName = 'Input'

export { Input }
