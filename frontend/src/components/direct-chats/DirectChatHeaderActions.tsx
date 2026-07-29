import { useState } from 'react'
import { RotateCcw, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  useClearConversationMessages,
  useResetDirectChatContext,
} from '@/hooks/useGroupMessages'

interface DirectChatHeaderActionsProps {
  chatId: string
  disabled?: boolean
}

export function DirectChatHeaderActions({
  chatId,
  disabled = false,
}: DirectChatHeaderActionsProps) {
  const { t } = useTranslation('chat')
  const clearMessages = useClearConversationMessages('direct-chats', chatId)
  const resetContext = useResetDirectChatContext(chatId)
  const [dialog, setDialog] = useState<'clear' | 'reset' | null>(null)
  const pending = clearMessages.isPending || resetContext.isPending

  return (
    <>
      <div className="mr-1 flex items-center gap-0.5 border-r border-border/60 pr-2">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="text-muted-foreground"
          disabled={disabled || pending}
          onClick={() => setDialog('reset')}
          aria-label={t('direct.resetContext')}
          title={t('direct.resetContext')}
        >
          <RotateCcw className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="text-muted-foreground hover:text-destructive"
          disabled={disabled || pending}
          onClick={() => setDialog('clear')}
          aria-label={t('direct.clearChat')}
          title={t('direct.clearChat')}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>

      <ConfirmDialog
        open={dialog === 'reset'}
        onOpenChange={(open) => setDialog(open ? 'reset' : null)}
        title={t('direct.resetContextTitle')}
        description={t('direct.resetContextDescription')}
        confirmLabel={t('direct.resetContext')}
        onConfirm={() => resetContext.mutateAsync()}
      />
      <ConfirmDialog
        open={dialog === 'clear'}
        onOpenChange={(open) => setDialog(open ? 'clear' : null)}
        title={t('direct.clearChatTitle')}
        description={t('direct.clearChatDescription')}
        confirmLabel={t('direct.clearChat')}
        destructive
        onConfirm={async () => {
          await clearMessages.mutateAsync()
        }}
      />
    </>
  )
}
