import { useMemo } from 'react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useAuthStore } from '@/stores/authStore'
import type { Message } from '@/types/api'
import { cn } from '@/lib/utils'

interface MessageItemProps {
  message: Message
  groupId: string
  isStreaming?: boolean
}

function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((s) => s[0]?.toUpperCase() ?? '')
    .join('')
}

export function MessageItem({ message, groupId, isStreaming }: MessageItemProps) {
  const groupAgents = useGroupAgents(groupId)
  const currentUser = useAuthStore((s) => s.user)

  const senderName = useMemo(() => {
    if (message.sender_type === 'user') {
      if (currentUser && message.sender_id === currentUser.id) return 'You'
      return 'User'
    }
    if (message.sender_type === 'agent') {
      const ga = groupAgents.data?.find((g) => g.agent_id === message.sender_id)
      return ga?.display_name ?? 'Agent'
    }
    return 'System'
  }, [currentUser, groupAgents.data, message.sender_id, message.sender_type])

  if (message.sender_type === 'system') {
    return (
      <div className="my-2 text-center text-xs text-muted-foreground">
        {message.content}
      </div>
    )
  }

  const isUser = message.sender_type === 'user'
  const time = new Date(message.created_at).toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
  })

  return (
    <div
      className={cn(
        'flex w-full gap-3 px-4 py-2',
        isUser ? 'flex-row-reverse' : 'flex-row',
      )}
    >
      <Avatar className="mt-0.5 h-8 w-8 shrink-0">
        <AvatarFallback>{initials(senderName)}</AvatarFallback>
      </Avatar>
      <div
        className={cn(
          'flex max-w-[78%] flex-col gap-1',
          isUser ? 'items-end' : 'items-start',
        )}
      >
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{senderName}</span>
          {!isStreaming && <span>{time}</span>}
          {isStreaming && (
            <span className="inline-flex items-center gap-1 text-amber-600">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />
              streaming
            </span>
          )}
        </div>
        <div
          className={cn(
            'whitespace-pre-wrap rounded-lg px-3 py-2 text-sm',
            isUser
              ? 'bg-primary text-primary-foreground'
              : 'border border-border bg-muted/40 text-foreground',
          )}
        >
          {message.content || ' '}
        </div>
      </div>
    </div>
  )
}
