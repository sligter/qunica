import { useState } from 'react'
import { ListPlus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { useStartNewGroupTask } from '@/hooks/useGroupMessages'

interface GroupChatHeaderActionsProps {
  groupId: string
  disabled?: boolean
}

export function GroupChatHeaderActions({
  groupId,
  disabled = false,
}: GroupChatHeaderActionsProps) {
  const { t } = useTranslation('groups')
  const startNewTask = useStartNewGroupTask(groupId)
  const [open, setOpen] = useState(false)

  return (
    <>
      <div className="mr-1 flex items-center border-r border-border/60 pr-2">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="text-muted-foreground"
          disabled={disabled || startNewTask.isPending}
          onClick={() => setOpen(true)}
          aria-label={t('actions.newTask')}
          title={t('actions.newTask')}
        >
          <ListPlus className="h-4 w-4" aria-hidden="true" />
        </Button>
      </div>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title={t('newTask.title')}
        description={t('newTask.description')}
        confirmLabel={t('actions.newTask')}
        onConfirm={() => startNewTask.mutateAsync()}
      />
    </>
  )
}
