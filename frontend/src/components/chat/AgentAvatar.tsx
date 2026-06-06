import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { cn } from '@/lib/utils'

type AvatarKind = 'agent' | 'user' | 'system'

interface AgentAvatarProps {
  name: string
  kind?: AvatarKind
  className?: string
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

export function AgentAvatar({ name, kind = 'agent', className }: AgentAvatarProps) {
  const fallbackClass =
    kind === 'user'
      ? 'bg-primary text-primary-foreground'
      : kind === 'system'
        ? 'bg-muted text-muted-foreground'
        : colorFor(name)
  return (
    <Avatar className={cn('h-8 w-8 shrink-0', className)}>
      <AvatarFallback className={cn('text-xs font-semibold', fallbackClass)}>
        {avatarInitials(name)}
      </AvatarFallback>
    </Avatar>
  )
}
