import type { ReactNode } from 'react'
import { Bot } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AgentAvatarArt } from '@/components/chat/AgentAvatarArt'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { formatNumber, formatTime } from '@/lib/format'
import { findAgentAvatarPreset, agentInitialsTone } from '@/lib/agentAvatar'
import { normalizeLanguage } from '@/i18n'
import type { ContextUsage } from '@/types/api'

type AvatarKind = 'agent' | 'user' | 'system'

interface AgentAvatarProps {
  name: string
  kind?: AvatarKind
  className?: string
  contextUsage?: ContextUsage | null
  avatarUrl?: string | null
  size?: 'sm' | 'md' | 'lg'
}

const AVATAR_SIZE = { sm: 'h-6 w-6', md: 'h-8 w-8', lg: 'h-12 w-12' } as const
const AVATAR_TEXT = { sm: 'text-[10px]', md: 'text-xs', lg: 'text-base' } as const

/** Up to two leading letters of a name, for the fallback initials avatar. */
export function avatarInitials(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return '?'
  const parts = trimmed.split(/\s+/).filter(Boolean)
  const letters = parts.slice(0, 2).map((part) => part[0]?.toUpperCase() ?? '')
  const joined = letters.join('')
  return joined || trimmed.slice(0, 2).toUpperCase()
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
    host_estimate: t('messages.context.hostEstimate'),
  }
  const sourceLabel = usage.source
    ? (sourceLabels[usage.source] ?? t('messages.context.sourceUnknownDetail', { value: usage.source }))
    : t('messages.context.sourceUnknown')

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
      <div className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-2xs text-muted-foreground">
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
  avatarUrl,
  size = 'md',
}: AgentAvatarProps) {
  const customizable = kind !== 'system'
  const preset = customizable ? findAgentAvatarPreset(avatarUrl) : undefined
  const imageUrl = customizable && avatarUrl?.startsWith('data:image/') ? avatarUrl : undefined
  const fallbackClass =
    kind === 'system'
      ? 'bg-muted text-muted-foreground'
      : preset
        ? 'bg-transparent'
        : kind === 'user'
          ? 'bg-primary text-primary-foreground'
          : agentInitialsTone(name)
  const ratio =
    kind !== 'user' && contextUsage?.ratio !== null && contextUsage?.ratio !== undefined
      ? Math.max(0, Math.min(1, contextUsage.ratio))
      : null

  const avatar = (
    <Avatar
      aria-label={name}
      className={cn(ratio === null ? AVATAR_SIZE[size] : 'h-7 w-7', 'shrink-0')}
    >
      {imageUrl && <AvatarImage src={imageUrl} alt="" className="object-cover" />}
      <AvatarFallback className={cn('font-semibold', AVATAR_TEXT[size], fallbackClass)}>
        {kind === 'system' ? (
          <Bot className="h-4 w-4" aria-hidden />
        ) : preset ? (
          <AgentAvatarArt preset={preset} />
        ) : (
          avatarInitials(name)
        )}
      </AvatarFallback>
    </Avatar>
  )

  let visual: ReactNode
  if (ratio === null) {
    visual = <span className={cn('inline-flex shrink-0', AVATAR_SIZE[size], className)}>{avatar}</span>
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

  // User avatars are never context meters; agent and Assistant avatars are.
  if (kind === 'user' || !contextUsage) return visual

  return (
    <Tooltip>
      <TooltipTrigger asChild>{visual}</TooltipTrigger>
      <TooltipContent side="right">
        <UsageTooltipBody usage={contextUsage} />
      </TooltipContent>
    </Tooltip>
  )
}
