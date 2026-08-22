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

/** Detail-page placeholder using the same measured header/content rhythm. */
export function DetailSkeleton({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-background" role="status">
      <span className="sr-only">{label}</span>
      <div className="flex min-h-14 shrink-0 items-center border-b border-border px-6 py-2.5">
        <div className="mx-auto flex w-full max-w-[1120px] items-center justify-between gap-4">
          <div className="space-y-1.5">
            <Skeleton className="h-4 w-40" />
            <Skeleton className="h-2.5 w-56 opacity-60" />
          </div>
          <Skeleton className="h-8 w-20" />
        </div>
      </div>
      <div className="mx-auto w-full max-w-[1120px] space-y-8 px-6 py-5" aria-hidden>
        <div className="grid gap-4 sm:grid-cols-2">
          <Skeleton className="h-16" />
          <Skeleton className="h-16" />
        </div>
        <div className="space-y-3">
          <Skeleton className="h-3 w-28" />
          <Skeleton className="h-3 w-full opacity-70" />
          <Skeleton className="h-3 w-5/6 opacity-60" />
        </div>
      </div>
    </div>
  )
}
