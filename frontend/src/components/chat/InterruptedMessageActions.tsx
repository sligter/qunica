import { Play } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useResumeStream } from '@/hooks/useResumeStream'

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
  const { resume, isStreaming, error } = useResumeStream(groupId, threadId, messageId)
  return (
    <div className="flex items-center gap-2 text-xs">
      <Button
        size="sm"
        variant="outline"
        onClick={resume}
        disabled={isStreaming}
        className="h-7 gap-1.5"
      >
        <Play className="h-3 w-3" />
        {t('messages.continue')}
      </Button>
      {error && <span className="text-destructive">{t('messages.resumeFailed', { message: error })}</span>}
    </div>
  )
}
