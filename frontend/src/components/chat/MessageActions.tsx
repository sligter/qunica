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
import {
  fetchConversationWorkspaceFileBlob,
  uploadConversationWorkspaceFile,
} from '@/hooks/useConversationWorkspaceFiles'
import { useGroups } from '@/hooks/useGroups'
import {
  type ConversationScope,
  useDeleteConversationMessage,
  useSendGroupMessage,
} from '@/hooks/useGroupMessages'
import { messageCopyText } from '@/lib/messageCopy'
import { useAuthStore } from '@/stores/authStore'
import type { MessageAttachment } from '@/types/api'

const NO_ATTACHMENTS: readonly MessageAttachment[] = []

interface MessageActionsProps {
  messageId: string
  content: string
  senderName: string
  timeLabel: string
  groupId: string
  threadId?: string | null
  scope?: ConversationScope
  /** Files the message was sent with; they travel with copy and share. */
  attachments?: readonly MessageAttachment[]
}

function shareText(senderName: string, timeLabel: string, content: string): string {
  return `${senderName} · ${timeLabel}\n\n${content}`.trim()
}

function errorDetail(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/**
 * Put the message on the clipboard, with its first image alongside the text.
 *
 * A clipboard holds one payload per flavour, so several images cannot all ride
 * along — the first one does, and the rest stay named in the text. Falls back to
 * text alone whenever the image flavour is unavailable or rejected: webviews
 * differ on which image types they accept, and losing the image is better than
 * losing the copy.
 */
async function copyMessage(
  text: string,
  image: MessageAttachment | undefined,
  loadImage: (attachment: MessageAttachment) => Promise<Blob>,
): Promise<void> {
  if (image && typeof ClipboardItem === 'function' && navigator.clipboard?.write) {
    const blob = loadImage(image)
    // `write` can reject before it ever consumes this promise, which would
    // leave the fetch failure unhandled.
    void blob.catch(() => null)
    try {
      await navigator.clipboard.write([
        new ClipboardItem({
          'text/plain': new Blob([text], { type: 'text/plain' }),
          [image.mime_type]: blob,
        }),
      ])
      return
    } catch {
      // Fall through to text.
    }
  }
  await navigator.clipboard.writeText(text)
}

/**
 * Re-attach this message's files to another group.
 *
 * The paths a message carries are relative to its own conversation workspace,
 * which the receiving group cannot read, so each file is fetched and uploaded
 * there under a free name. Sequential: two uploads racing for the same name
 * would have the server hand out the same free one twice.
 */
async function copyAttachmentsToGroup(
  attachments: readonly MessageAttachment[],
  scope: ConversationScope,
  sourceConversationId: string,
  targetGroupId: string,
  token: string | null,
): Promise<Array<Pick<MessageAttachment, 'path'>>> {
  const copied: Array<Pick<MessageAttachment, 'path'>> = []
  for (const attachment of attachments) {
    const blob = await fetchConversationWorkspaceFileBlob(
      scope,
      sourceConversationId,
      attachment.path,
      token,
    )
    const uploaded = await uploadConversationWorkspaceFile(
      'groups',
      targetGroupId,
      new File([blob], attachment.name, { type: attachment.mime_type || blob.type }),
      token,
      null,
      { uniqueName: true },
    )
    copied.push({ path: uploaded.path })
  }
  return copied
}

interface ShareMessageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  content: string
  groupId: string
  attachments: readonly MessageAttachment[]
  scope: ConversationScope
}

/**
 * Share picker body. Mounted only while open so the `groups` query and its cache
 * subscription cost one instance per open dialog, not one per rendered message.
 */
function ShareMessageDialog({
  open,
  onOpenChange,
  content,
  groupId,
  attachments,
  scope,
}: ShareMessageDialogProps) {
  const { t } = useTranslation(['chat', 'common'])
  const groups = useGroups()
  const token = useAuthStore((state) => state.token)
  const sendGroupMessage = useSendGroupMessage()
  const [shareError, setShareError] = useState<string | null>(null)
  // Copying attachments happens outside the mutation, so `isPending` alone
  // would leave the buttons live through the uploads.
  const [sharingGroupId, setSharingGroupId] = useState<string | null>(null)
  const targetGroups = groups.data?.filter((group) => group.id !== groupId) ?? []
  const sharing = sharingGroupId !== null || sendGroupMessage.isPending

  const shareToGroup = async (targetGroupId: string) => {
    if (sharing) return
    setShareError(null)
    setSharingGroupId(targetGroupId)
    try {
      const copied = await copyAttachmentsToGroup(
        attachments,
        scope,
        groupId,
        targetGroupId,
        token,
      )
      await sendGroupMessage.mutateAsync({
        groupId: targetGroupId,
        content,
        attachments: copied,
      })
      onOpenChange(false)
    } catch (error) {
      setShareError(errorDetail(error))
    } finally {
      setSharingGroupId(null)
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
              disabled={sharing}
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

        {attachments.length > 0 && sharingGroupId !== null ? (
          <p className="text-sm text-muted-foreground">
            {t('messages.shareCopyingAttachments', { ns: 'chat', count: attachments.length })}
          </p>
        ) : null}

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
  threadId?: string | null
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
  threadId,
  scope,
}: DeleteMessageConfirmProps) {
  const { t } = useTranslation(['chat', 'common'])
  const deleteMessage = useDeleteConversationMessage(scope, groupId, threadId)

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
  threadId,
  scope = 'groups',
  attachments = NO_ATTACHMENTS,
}: MessageActionsProps) {
  const { t } = useTranslation(['chat', 'common'])
  const token = useAuthStore((state) => state.token)
  const [copied, setCopied] = useState(false)
  const [shareOpen, setShareOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)

  const copy = async () => {
    await copyMessage(
      messageCopyText(content, attachments, t('messages.copyAttachments', { ns: 'chat' })),
      attachments.find((attachment) => attachment.kind === 'image'),
      (attachment) =>
        fetchConversationWorkspaceFileBlob(scope, groupId, attachment.path, token),
    )
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
          attachments={attachments}
          scope={scope}
        />
      ) : null}

      {deleteOpen ? (
        <DeleteMessageConfirm
          open
          onOpenChange={setDeleteOpen}
          messageId={messageId}
          groupId={groupId}
          threadId={threadId}
          scope={scope}
        />
      ) : null}
    </>
  )
}
