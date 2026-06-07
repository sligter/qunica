import type { ReactNode } from 'react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import type { ContextUsage } from '@/types/api'

type AvatarKind = 'agent' | 'user' | 'system'

interface AgentAvatarProps {
  name: string
  kind?: AvatarKind
  className?: string
  contextUsage?: ContextUsage | null
}

/** Deterministic, readable color pairs for agent initials avatars. */
const AGENT_PALETTE = [
  'bg-blue-100 text-blue-700',
  'bg-emerald-100 text-emerald-700',
  'bg-violet-100 text-violet-700',
  'bg-amber-100 text-amber-700',
  'bg-rose-100 text-rose-700',
  'bg-cyan-100 text-cyan-700',
  'bg-indigo-100 text-indigo-700',
  'bg-teal-100 text-teal-700',
  'bg-fuchsia-100 text-fuchsia-700',
  'bg-lime-100 text-lime-700',
]

export function avatarInitials(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return '?'
  const parts = trimmed.split(/\s+/).filter(Boolean)
  const letters = parts.slice(0, 2).map((part) => part[0]?.toUpperCase() ?? '')
  const joined = letters.join('')
  return joined || trimmed.slice(0, 2).toUpperCase()
}

function colorFor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i += 1) {
    hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  }
  return AGENT_PALETTE[hash % AGENT_PALETTE.length]
}

/** Ring/badge color by how full the context window is. */
function usageColor(ratio: number): string {
  if (ratio >= 0.9) return 'hsl(0 72% 51%)'
  if (ratio >= 0.75) return 'hsl(38 92% 50%)'
  return 'hsl(160 84% 39%)'
}

function formatTokens(value: number | null | undefined): string {
  return typeof value === 'number' ? value.toLocaleString() : '—'
}

function usageSourceLabel(source: string | null | undefined): string {
  switch (source) {
    case 'provider':
      return 'Reported by provider'
    case 'previous_provider_delta':
      return 'Previous provider + estimate'
    case 'fallback_tokenizer':
      return 'Estimated (tokenizer)'
    default:
      return source ? source : 'Source unknown'
  }
}

function formatUpdatedAt(value: string | null | undefined): string | null {
  if (!value) return null
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) return null
  return parsed.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

/** Rich hover card describing one agent's context-window usage. */
function UsageTooltipBody({ usage }: { usage: ContextUsage }) {
  const ratio =
    usage.ratio !== null && usage.ratio !== undefined
      ? Math.max(0, Math.min(1, usage.ratio))
      : null
  const dotColor = ratio !== null ? usageColor(ratio) : 'var(--color-muted-foreground)'
  const percentLabel = ratio !== null ? `${Math.round(ratio * 100)}%` : 'Usage unknown'
  const hasOutput =
    typeof usage.output_tokens === 'number' || typeof usage.total_tokens === 'number'
  const updatedLabel = formatUpdatedAt(usage.updated_at)

  return (
    <div className="flex min-w-[11rem] flex-col gap-1.5">
      <div className="flex items-center gap-1.5 font-semibold text-foreground">
        <span
          aria-hidden="true"
          className="inline-block h-2 w-2 shrink-0 rounded-full"
          style={{ background: dotColor }}
        />
        <span>Context {percentLabel}</span>
      </div>
      <div className="text-muted-foreground">
        <span className="font-medium text-foreground">{formatTokens(usage.input_tokens)}</span>
        {' / '}
        {formatTokens(usage.context_window_tokens)} tokens
      </div>
      {hasOutput && (
        <div className="text-muted-foreground">
          Output {formatTokens(usage.output_tokens)} · Total {formatTokens(usage.total_tokens)}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[11px] text-muted-foreground">
        <span>{usageSourceLabel(usage.source)}</span>
        {updatedLabel && (
          <>
            <span aria-hidden="true">·</span>
            <span>Updated {updatedLabel}</span>
          </>
        )}
      </div>
    </div>
  )
}

export function AgentAvatar({
  name,
  kind = 'agent',
  className,
  contextUsage,
}: AgentAvatarProps) {
  const fallbackClass =
    kind === 'user'
      ? 'bg-primary text-primary-foreground'
      : kind === 'system'
        ? 'bg-muted text-muted-foreground'
        : colorFor(name)
  const ratio =
    kind === 'agent' && contextUsage?.ratio !== null && contextUsage?.ratio !== undefined
      ? Math.max(0, Math.min(1, contextUsage.ratio))
      : null

  const avatar = (
    <Avatar className={cn(ratio === null ? 'h-8 w-8' : 'h-7 w-7', 'shrink-0')}>
      <AvatarFallback className={cn('text-xs font-semibold', fallbackClass)}>
        {avatarInitials(name)}
      </AvatarFallback>
    </Avatar>
  )

  let visual: ReactNode
  if (ratio === null) {
    visual = <span className={cn('inline-flex h-8 w-8 shrink-0', className)}>{avatar}</span>
  } else {
    visual = (
      <span
        className={cn(
          'relative inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-full p-[3px] shadow-sm',
          className,
        )}
        style={{
          background: `conic-gradient(${usageColor(ratio)} ${Math.round(
            ratio * 360,
          )}deg, var(--color-border) 0deg)`,
        }}
      >
        <span className="flex h-full w-full items-center justify-center rounded-full bg-background">
          {avatar}
        </span>
      </span>
    )
  }

  // Only agents with usage data get the interactive hover card.
  if (kind !== 'agent' || !contextUsage) return visual

  return (
    <Tooltip>
      <TooltipTrigger asChild>{visual}</TooltipTrigger>
      <TooltipContent side="right">
        <UsageTooltipBody usage={contextUsage} />
      </TooltipContent>
    </Tooltip>
  )
}
