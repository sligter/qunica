import { Check, Copy, Share2, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useGroups } from '@/hooks/useGroups'
import {
  type ConversationScope,
  useDeleteConversationMessage,
  useSendGroupMessage,
} from '@/hooks/useGroupMessages'

interface MessageActionsProps {
  messageId: string
  content: string
  senderName: string
  timeLabel: string
  groupId: string
  scope?: ConversationScope
}

function shareText(senderName: string, timeLabel: string, content: string): string {
  return `${senderName} · ${timeLabel}\n\n${content}`.trim()
}

function errorDetail(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

interface ShareMessageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  content: string
  groupId: string
}

/**
 * Share picker body. Mounted only while open so the `groups` query and its cache
 * subscription cost one instance per open dialog, not one per rendered message.
 */
function ShareMessageDialog({ open, onOpenChange, content, groupId }: ShareMessageDialogProps) {
  const { t } = useTranslation(['chat', 'common'])
  const groups = useGroups()
  const sendGroupMessage = useSendGroupMessage()
  const [shareError, setShareError] = useState<string | null>(null)
  const targetGroups = groups.data?.filter((group) => group.id !== groupId) ?? []

  const shareToGroup = async (targetGroupId: string) => {
    setShareError(null)
    try {
      await sendGroupMessage.mutateAsync({ groupId: targetGroupId, content })
      onOpenChange(false)
    } catch (error) {
      setShareError(errorDetail(error))
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent closeLabel={t('actions.close', { ns: 'common' })} className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('messages.shareTitle', { ns: 'chat' })}</DialogTitle>
          <DialogDescription>{t('messages.shareDescription', { ns: 'chat' })}</DialogDescription>
        </DialogHeader>

        <div className="max-h-72 space-y-2 overflow-y-auto">
          {groups.isLoading ? (
            <p className="text-sm text-muted-foreground">{t('messages.loadingGroups', { ns: 'chat' })}</p>
          ) : null}
          {!groups.isLoading && targetGroups.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('messages.noOtherGroups', { ns: 'chat' })}</p>
          ) : null}
          {targetGroups.map((group) => (
            <Button
              key={group.id}
              type="button"
              variant="outline"
              className="h-auto w-full justify-start px-3 py-2 text-left"
              disabled={sendGroupMessage.isPending}
              onClick={() => void shareToGroup(group.id)}
            >
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium">{group.name}</span>
                {group.description ? (
                  <span className="block truncate text-xs text-muted-foreground">{group.description}</span>
                ) : null}
              </span>
            </Button>
          ))}
        </div>

        {shareError ? (
          <p className="text-sm text-destructive">
            {t('messages.shareFailedDetail', { ns: 'chat', message: shareError })}
          </p>
        ) : null}

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t('actions.cancel', { ns: 'common' })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

interface DeleteMessageConfirmProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  messageId: string
  groupId: string
  scope: ConversationScope
}

/**
 * Delete confirmation. Mounted only while open, so the delete mutation is
 * registered once per pending confirmation instead of once per message row.
 */
function DeleteMessageConfirm({
  open,
  onOpenChange,
  messageId,
  groupId,
  scope,
}: DeleteMessageConfirmProps) {
  const { t } = useTranslation(['chat', 'common'])
  const deleteMessage = useDeleteConversationMessage(scope, groupId)

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('messages.deleteTitle', { ns: 'chat' })}
      description={t('messages.deleteDescription', { ns: 'chat' })}
      confirmLabel={t('messages.delete', { ns: 'chat' })}
      destructive
      onConfirm={async () => {
        try {
          await deleteMessage.mutateAsync({ messageId })
        } catch (error) {
          throw new Error(
            t('messages.deleteFailedDetail', { ns: 'chat', message: errorDetail(error) }),
          )
        }
      }}
    />
  )
}

/**
 * Hover-revealed per-message actions. Holds no server-state subscriptions of its
 * own — the share picker and delete confirmation mount on demand.
 */
export function MessageActions({
  messageId,
  content,
  senderName,
  timeLabel,
  groupId,
  scope = 'groups',
}: MessageActionsProps) {
  const { t } = useTranslation(['chat', 'common'])
  const [copied, setCopied] = useState(false)
  const [shareOpen, setShareOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)

  const copy = async () => {
    await navigator.clipboard.writeText(content)
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1200)
  }

  return (
    <>
      <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover/message:opacity-100 focus-within:opacity-100">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground hover:text-foreground"
          onClick={() => void copy()}
          aria-label={t('messages.copyMessage', { ns: 'chat' })}
        >
          {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground hover:text-foreground"
          onClick={() => setShareOpen(true)}
          aria-label={t('messages.share', { ns: 'chat' })}
        >
          <Share2 className="h-3.5 w-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground hover:text-destructive"
          onClick={() => setDeleteOpen(true)}
          aria-label={t('messages.delete', { ns: 'chat' })}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>

      {shareOpen ? (
        <ShareMessageDialog
          open
          onOpenChange={setShareOpen}
          content={shareText(senderName, timeLabel, content)}
          groupId={groupId}
        />
      ) : null}

      {deleteOpen ? (
        <DeleteMessageConfirm
          open
          onOpenChange={setDeleteOpen}
          messageId={messageId}
          groupId={groupId}
          scope={scope}
        />
      ) : null}
    </>
  )
}
