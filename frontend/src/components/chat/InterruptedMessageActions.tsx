import { Play } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useResumeStream } from '@/hooks/useResumeStream'
import { MAX_RETRY_ATTEMPTS } from '@/lib/api-v2/retry'

interface InterruptedMessageActionsProps {
  groupId: string
  threadId: string
  messageId: string
}

export function InterruptedMessageActions({
  groupId,
  threadId,
  messageId,
}: InterruptedMessageActionsProps) {
  const { t } = useTranslation('chat')
  const { resume, isStreaming, error, retry, retryExhausted } = useResumeStream(
    groupId,
    threadId,
    messageId,
  )
  if (isStreaming && !retry) return null

  return (
    <div className="flex items-center gap-2 text-xs">
      {!isStreaming ? (
        <Button size="sm" variant="outline" onClick={resume} className="h-7 gap-1.5">
          <Play className="h-3 w-3" />
          {t('messages.continue')}
        </Button>
      ) : null}
      {retry ? (
        <span role="status" aria-live="polite" className="text-warning-foreground">
          {t('stream.reconnecting', { attempt: retry.attempt, max: MAX_RETRY_ATTEMPTS })}
        </span>
      ) : null}
      {!isStreaming && error ? (
        <span className="text-destructive">
          {retryExhausted
            ? t('stream.retryExhausted', { max: MAX_RETRY_ATTEMPTS })
            : t('messages.resumeFailed', { message: error })}
        </span>
      ) : null}
    </div>
  )
}
