import { cn } from '@/lib/utils'

/** One shimmering placeholder bar. Compose loading rows out of these. */
export function Skeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cn('stream-skeleton rounded-md bg-muted', className)}
    />
  )
}

/**
 * Placeholder for a list column while its query is in flight: the same
 * avatar/two-line row shape the real list renders, so nothing jumps when data
 * arrives. Purely decorative — the shimmer reads as "loading" without being
 * announced over the actual content.
 */
export function SkeletonList({
  rows = 6,
  className,
}: {
  rows?: number
  className?: string
}) {
  return (
    <ul className={cn('space-y-0.5', className)} aria-hidden>
      {Array.from({ length: rows }, (_, index) => (
        <li key={index} className="flex items-start gap-2.5 rounded-lg px-2.5 py-1.5">
          <Skeleton className="mt-0.5 h-8 w-8 shrink-0 rounded-full" />
          <div className="flex min-w-0 flex-1 flex-col gap-1.5 pt-1">
            <Skeleton className="h-3 w-2/3" />
            <Skeleton className="h-2.5 w-11/12 opacity-60" />
          </div>
        </li>
      ))}
    </ul>
  )
}
