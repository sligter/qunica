import { useEffect, useRef, type ReactNode } from 'react'
import { Ellipsis } from 'lucide-react'
import { useCompactLayout } from '@/hooks/useMediaQuery'

/** Keep dialog state mounted while tucking secondary actions into a touch menu. */
export function CompactActions({ label, children, icon }: { label: string; children: ReactNode; icon?: ReactNode }) {
  const compact = useCompactLayout()
  const ref = useRef<HTMLDetailsElement>(null)
  useEffect(() => {
    if (!compact) return
    const dismiss = (event: PointerEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) ref.current.open = false
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && ref.current?.open) {
        ref.current.open = false
        ref.current.querySelector('summary')?.focus()
      }
    }
    document.addEventListener('pointerdown', dismiss)
    document.addEventListener('keydown', escape)
    return () => {
      document.removeEventListener('pointerdown', dismiss)
      document.removeEventListener('keydown', escape)
    }
  }, [compact])
  if (!compact) return <div className="flex shrink-0 items-center gap-1">{children}</div>
  return <details ref={ref} className="compact-actions relative shrink-0">
    <summary aria-label={label} title={label} className="flex h-11 w-11 cursor-pointer list-none items-center justify-center rounded-xl text-muted-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">{icon ?? <Ellipsis className="h-5 w-5" />}</summary>
    <div className="compact-action-list absolute right-0 top-full z-30 mt-1 w-60 max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-popover p-2 shadow-xl" onClick={event => {
      const target = event.target as Element
      if (event.currentTarget.contains(target) && target.closest('button,a') && ref.current) ref.current.open = false
    }}>{children}</div>
  </details>
}
