import { Check, Copy, Share2, Trash2 } from 'lucide-react'
import { useState } from 'react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useGroups } from '@/hooks/useGroups'
import { useDeleteGroupMessage, useSendGroupMessage } from '@/hooks/useGroupMessages'
import { ApiError } from '@/lib/api-v2/client'

interface MessageActionsProps {
  messageId: string
  content: string
  senderName: string
  timeLabel: string
  groupId: string
}

type CopiedAction = 'message' | null

function shareText(senderName: string, timeLabel: string, content: string): string {
  return `${senderName} · ${timeLabel}\n\n${content}`.trim()
}

export function MessageActions({
  messageId,
  content,
  senderName,
  timeLabel,
  groupId,
}: MessageActionsProps) {
  const [copiedAction, setCopiedAction] = useState<CopiedAction>(null)
  const [shareOpen, setShareOpen] = useState(false)
  const [shareError, setShareError] = useState<string | null>(null)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const groups = useGroups()
  const sendGroupMessage = useSendGroupMessage()
  const deleteGroupMessage = useDeleteGroupMessage(groupId)
  const shareContent = shareText(senderName, timeLabel, content)
  const targetGroups = groups.data?.filter((group) => group.id !== groupId) ?? []

  const copy = async (action: Exclude<CopiedAction, null>, text: string) => {
    await navigator.clipboard.writeText(text)
    setCopiedAction(action)
    window.setTimeout(() => setCopiedAction(null), 1200)
  }

  const shareToGroup = async (targetGroupId: string) => {
    setShareError(null)
    try {
      await sendGroupMessage.mutateAsync({ groupId: targetGroupId, content: shareContent })
      setShareOpen(false)
    } catch (error) {
      setShareError(error instanceof Error ? error.message : 'Share failed')
    }
  }

  const deleteMessage = async () => {
    setDeleteError(null)
    try {
      await deleteGroupMessage.mutateAsync({ messageId })
    } catch (error) {
      setDeleteError(error instanceof ApiError ? error.message : 'Delete failed')
    }
  }

  return (
    <>
      <div className="flex flex-col items-end gap-1">
        <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover/message:opacity-100 focus-within:opacity-100">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-foreground"
            onClick={() => void copy('message', content)}
            aria-label="Copy message"
          >
            {copiedAction === 'message' ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-foreground"
            onClick={() => setShareOpen(true)}
            aria-label="Share message to group"
          >
            <Share2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-destructive"
            disabled={deleteGroupMessage.isPending}
            onClick={() => void deleteMessage()}
            aria-label="Delete message"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
        {deleteError ? (
          <p className="max-w-44 text-right text-xs text-destructive">{deleteError}</p>
        ) : null}
      </div>

      <Dialog open={shareOpen} onOpenChange={setShareOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Share to group chat</DialogTitle>
            <DialogDescription>Choose another group to receive this message.</DialogDescription>
          </DialogHeader>

          <div className="max-h-72 space-y-2 overflow-y-auto">
            {groups.isLoading ? <p className="text-sm text-muted-foreground">Loading groups…</p> : null}
            {!groups.isLoading && targetGroups.length === 0 ? (
              <p className="text-sm text-muted-foreground">No other groups available.</p>
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

          {shareError ? <p className="text-sm text-destructive">{shareError}</p> : null}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setShareOpen(false)}>
              Cancel
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
