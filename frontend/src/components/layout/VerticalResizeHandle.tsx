import type { KeyboardEvent, PointerEvent } from 'react'

import { cn } from '@/lib/utils'

interface VerticalResizeHandleProps {
  label: string
  value: number
  min: number
  max: number
  onResizeStart: (event: PointerEvent<HTMLButtonElement>) => void
  onStep: (delta: number) => void
  increaseOnArrowRight?: boolean
  className?: string
}

const KEYBOARD_STEP = 16

export function VerticalResizeHandle({
  label,
  value,
  min,
  max,
  onResizeStart,
  onStep,
  increaseOnArrowRight = true,
  className,
}: VerticalResizeHandleProps) {
  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    const direction = event.key === 'ArrowRight' ? 1 : -1
    onStep(direction * KEYBOARD_STEP * (increaseOnArrowRight ? 1 : -1))
  }

  return (
    <button
      type="button"
      role="separator"
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      title={label}
      className={cn(
        'group relative z-10 h-full w-2 shrink-0 cursor-col-resize touch-none bg-transparent outline-none transition-colors hover:bg-primary/10 focus-visible:bg-primary/15 focus-visible:ring-1 focus-visible:ring-ring',
        className,
      )}
      onPointerDown={onResizeStart}
      onKeyDown={handleKeyDown}
    >
      <span className="absolute left-1/2 top-0 h-full w-px -translate-x-1/2 bg-border transition-colors group-hover:bg-primary/45 group-focus-visible:bg-primary/60" />
    </button>
  )
}
