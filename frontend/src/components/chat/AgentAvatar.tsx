import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { formatNumber, formatTime } from '@/lib/format'
import { normalizeLanguage } from '@/i18n'
import type { ContextUsage } from '@/types/api'

type AvatarKind = 'agent' | 'user' | 'system'

interface AgentAvatarProps {
  name: string
  kind?: AvatarKind
  className?: string
  contextUsage?: ContextUsage | null
}

/** Deterministic, readable color pairs for agent initials avatars (warm token palette). */
const AGENT_PALETTE = [
  'bg-avatar-1/15 text-avatar-1',
  'bg-avatar-2/15 text-avatar-2',
  'bg-avatar-3/15 text-avatar-3',
  'bg-avatar-4/15 text-avatar-4',
  'bg-avatar-5/15 text-avatar-5',
  'bg-avatar-6/15 text-avatar-6',
  'bg-avatar-7/15 text-avatar-7',
  'bg-avatar-8/15 text-avatar-8',
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
  if (ratio >= 0.9) return 'var(--color-destructive)'
  if (ratio >= 0.75) return 'var(--color-warning-foreground)'
  return 'var(--color-success)'
}

function formatTokens(value: number | null | undefined, language: 'en-US' | 'zh-CN'): string {
  return typeof value === 'number' ? formatNumber(value, language) : '—'
}

function formatUpdatedAt(
  value: string | null | undefined,
  language: 'en-US' | 'zh-CN',
): string | null {
  if (!value) return null
  const parsed = new Date(value)
  return Number.isNaN(parsed.getTime()) ? null : formatTime(parsed, language)
}

/** Rich hover card describing one agent's context-window usage. */
function UsageTooltipBody({ usage }: { usage: ContextUsage }) {
  const { t, i18n } = useTranslation('chat')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const ratio =
    usage.ratio !== null && usage.ratio !== undefined
      ? Math.max(0, Math.min(1, usage.ratio))
      : null
  const dotColor = ratio !== null ? usageColor(ratio) : 'var(--color-muted-foreground)'
  const percentLabel = ratio !== null
    ? `${formatNumber(Math.round(ratio * 100), language)}%`
    : t('messages.context.usageUnknown')
  const hasOutput =
    typeof usage.output_tokens === 'number' || typeof usage.total_tokens === 'number'
  const updatedLabel = formatUpdatedAt(usage.updated_at, language)
  const sourceLabels: Record<string, string> = {
    provider: t('messages.context.provider'),
    previous_provider_delta: t('messages.context.previousProvider'),
    fallback_tokenizer: t('messages.context.estimated'),
  }
  const sourceLabel = usage.source ? (sourceLabels[usage.source] ?? usage.source) : t('messages.context.sourceUnknown')

  return (
    <div className="flex min-w-[11rem] flex-col gap-1.5">
      <div className="flex items-center gap-1.5 font-semibold text-foreground">
        <span
          aria-hidden="true"
          className="inline-block h-2 w-2 shrink-0 rounded-full"
          style={{ background: dotColor }}
        />
        <span>{t('messages.context.title', { usage: percentLabel })}</span>
      </div>
      <div className="text-muted-foreground">
        <span className="font-medium text-foreground">{formatTokens(usage.input_tokens, language)}</span>
        {' / '}
        {t('messages.context.tokens', { count: formatTokens(usage.context_window_tokens, language) })}
      </div>
      {hasOutput && (
        <div className="text-muted-foreground">
          {t('messages.context.outputTotal', { output: formatTokens(usage.output_tokens, language), total: formatTokens(usage.total_tokens, language) })}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[11px] text-muted-foreground">
        <span>{sourceLabel}</span>
        {updatedLabel && (
          <>
            <span aria-hidden="true">·</span>
            <span>{t('messages.context.updated', { time: updatedLabel })}</span>
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
