import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'

interface ProseBlockProps {
  /** Preserves authored line breaks — for prompts, markdown bodies, logs. */
  children: ReactNode
  /** Caps the height and scrolls past it, so one long body can't own the page. */
  maxHeight?: 'none' | 'md' | 'lg'
  className?: string
}

const MAX_HEIGHT_CLASS: Record<'none' | 'md' | 'lg', string> = {
  none: '',
  md: 'max-h-72 overflow-y-auto',
  lg: 'max-h-[32rem] overflow-y-auto',
}

/**
 * A read-only block of authored text on the card surface: system prompts, skill
 * bodies, descriptions. One container so every long-form excerpt in the app has
 * the same padding, border and measure.
 */
export function ProseBlock({ children, maxHeight = 'md', className }: ProseBlockProps) {
  return (
    <pre
      className={cn(
        'whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 font-sans text-sm leading-relaxed text-foreground',
        MAX_HEIGHT_CLASS[maxHeight],
        className,
      )}
    >
      {children}
    </pre>
  )
}
