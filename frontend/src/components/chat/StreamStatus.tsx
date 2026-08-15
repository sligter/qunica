/**
 * The one vocabulary for "the agent is working on it".
 *
 * A reply passes through distinct phases — getting ready, thinking, running a
 * tool, writing, waiting on the user — and a single undifferentiated pulsing
 * dot said none of that. Each phase gets its own icon, wording and colour, so
 * a glance answers *what* is happening, and the elapsed counter answers *how
 * long*, which is the question a silent wait actually raises.
 */

import { useEffect, useState } from 'react'
import { Brain, PauseCircle, PenLine, Sparkles, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import '@/i18n'
import { cn } from '@/lib/utils'

/** What an agent is doing now, in the order a turn moves through. */
export type StreamPhase = 'preparing' | 'thinking' | 'tool' | 'writing' | 'waiting'

export interface StreamStatus {
  phase: StreamPhase
  /** Tool or adapter name, when the phase is a tool call. */
  detail?: string
  /** When the phase started, for the elapsed counter. */
  since?: string
}

const PHASE_ICONS = {
  preparing: Sparkles,
  thinking: Brain,
  tool: Wrench,
  writing: PenLine,
  waiting: PauseCircle,
} as const satisfies Record<StreamPhase, typeof Sparkles>

function elapsedSeconds(startedAt: number): number {
  return Math.max(0, Math.round((Date.now() - startedAt) / 1000))
}

/**
 * Whole seconds since `since`, ticking once a second, and null until it passes
 * `minimum`. A counter on a reply that lands in under two seconds is noise.
 */
function useElapsed(since: string | undefined, minimum: number): number | null {
  const startedAt = since ? Date.parse(since) : Number.NaN
  const valid = Number.isFinite(startedAt)
  const [seconds, setSeconds] = useState(() => (valid ? elapsedSeconds(startedAt) : 0))

  useEffect(() => {
    if (!valid) return
    setSeconds(elapsedSeconds(startedAt))
    const timer = window.setInterval(() => setSeconds(elapsedSeconds(startedAt)), 1_000)
    return () => window.clearInterval(timer)
  }, [startedAt, valid])

  return valid && seconds >= minimum ? seconds : null
}

/** Three dots riding a wave — the indeterminate "still going" beat. */
export function ThinkingDots({ className }: { className?: string }) {
  return (
    <span aria-hidden="true" className={cn('inline-flex shrink-0 items-center gap-[3px]', className)}>
      <span className="animate-stream-dot h-1 w-1 rounded-full bg-current" />
      <span className="animate-stream-dot h-1 w-1 rounded-full bg-current [animation-delay:150ms]" />
      <span className="animate-stream-dot h-1 w-1 rounded-full bg-current [animation-delay:300ms]" />
    </span>
  )
}

interface StreamStatusPillProps {
  status: StreamStatus
  /** Replaces the phase wording where the caller has something more precise. */
  label?: string
  className?: string
}

/**
 * The status chip that sits where a timestamp would once the reply lands, so
 * the header never reflows between "working" and "done".
 */
export function StreamStatusPill({ status, label, className }: StreamStatusPillProps) {
  const { t } = useTranslation('chat')
  const Icon = PHASE_ICONS[status.phase]
  const seconds = useElapsed(status.since, 2)
  const waiting = status.phase === 'waiting'
  const text =
    label ??
    (status.phase === 'tool'
      ? status.detail
        ? t('stream.phase.tool', { name: status.detail })
        : t('stream.phase.toolGeneric')
      : t(`stream.phase.${status.phase}`))

  return (
    <span
      role="status"
      className={cn(
        'relative inline-flex min-w-0 max-w-full items-center gap-1.5 overflow-hidden rounded-full border',
        'px-2 py-[3px] text-[11px] font-medium leading-none',
        waiting
          ? 'border-warning-foreground/25 bg-warning text-warning-foreground'
          : 'border-primary/25 bg-primary/10 text-primary',
        className,
      )}
    >
      {waiting ? null : (
        <span
          aria-hidden="true"
          className="animate-stream-sheen pointer-events-none absolute inset-y-0 left-0 w-1/3 bg-gradient-to-r from-transparent via-current to-transparent opacity-[0.14]"
        />
      )}
      <Icon
        aria-hidden="true"
        className={cn('h-3 w-3 shrink-0', waiting ? null : 'animate-stream-breathe')}
      />
      <span className="min-w-0 truncate">{text}</span>
      {waiting ? null : <ThinkingDots />}
      {seconds === null ? null : (
        <span className="shrink-0 tabular-nums opacity-60">{t('stream.elapsed', { seconds })}</span>
      )}
    </span>
  )
}

const SKELETON_WIDTHS = ['w-[62%]', 'w-[88%]', 'w-[45%]'] as const

/**
 * Shimmering placeholder lines standing in for the reply that has not started
 * arriving. It reserves the shape of an answer, so the block does not jump
 * when the first token lands.
 */
export function StreamSkeleton({ lines = 2, className }: { lines?: number; className?: string }) {
  return (
    <div aria-hidden="true" className={cn('w-full max-w-md space-y-2 py-1', className)}>
      {Array.from({ length: lines }, (_, index) => (
        <span
          key={index}
          className={cn('stream-skeleton block h-2 rounded-full', SKELETON_WIDTHS[index % SKELETON_WIDTHS.length])}
          style={{ animationDelay: `${index * 120}ms` }}
        />
      ))}
    </div>
  )
}
