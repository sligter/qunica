/**
 * The one dot that says what a conversation is doing.
 *
 * Both lists that name conversations — the sidebar and the group task switcher
 * — show it, so "this one is still working" reads the same in either place. It
 * borrows the reply-status motion vocabulary rather than inventing a blink of
 * its own, which also means it settles quietly under reduced motion.
 */

import { useTranslation } from 'react-i18next'

import { useConversationStatus, useThreadStatus } from '@/hooks/useConversationActivity'
import { cn } from '@/lib/utils'
import type { ConversationActivityStatus } from '@/stores/conversationActivityStore'

const STATUS_CLASSES = {
  running: 'bg-primary animate-stream-breathe',
  waiting: 'bg-warning-foreground',
  failed: 'bg-destructive',
} as const satisfies Record<ConversationActivityStatus, string>

const LABEL_CLASSES = {
  running: 'text-primary',
  waiting: 'text-warning-foreground',
  failed: 'text-destructive',
} as const satisfies Record<ConversationActivityStatus, string>

export interface ConversationStatusDotProps {
  status: ConversationActivityStatus | null | undefined
  /** Spell the status out next to the dot, where there is room for words. */
  showLabel?: boolean
  className?: string
}

export function ConversationStatusDot({
  status,
  showLabel = false,
  className,
}: ConversationStatusDotProps) {
  const { t } = useTranslation('chat')
  if (!status) return null
  const label = t(`conversationStatus.${status}`)

  return (
    <span
      // Deliberately not a live region: the status belongs to the name of the
      // row it sits in, and twenty conversations announcing every change would
      // bury the one the user asked about.
      title={label}
      className={cn(
        'inline-flex shrink-0 items-center gap-1 text-[10px] font-medium leading-none',
        showLabel ? LABEL_CLASSES[status] : null,
        className,
      )}
    >
      <span
        aria-hidden="true"
        className={cn('h-1.5 w-1.5 shrink-0 rounded-full', STATUS_CLASSES[status])}
      />
      <span className={showLabel ? 'truncate' : 'sr-only'}>{label}</span>
    </span>
  )
}

interface ConversationIndicatorProps {
  conversationId: string | undefined
  showLabel?: boolean
  className?: string
}

/** The status of a whole conversation, every task thread in it included. */
export function ConversationStatusIndicator({
  conversationId,
  showLabel,
  className,
}: ConversationIndicatorProps) {
  return (
    <ConversationStatusDot
      status={useConversationStatus(conversationId)}
      showLabel={showLabel}
      className={className}
    />
  )
}

/** The status of a single task thread. */
export function ThreadStatusIndicator({
  conversationId,
  threadId,
  showLabel,
  className,
}: ConversationIndicatorProps & { threadId: string | undefined }) {
  return (
    <ConversationStatusDot
      status={useThreadStatus(conversationId, threadId)}
      showLabel={showLabel}
      className={className}
    />
  )
}
