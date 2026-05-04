import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Separator } from '@/components/ui/separator'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useDeleteGroup } from '@/hooks/useDeleteGroup'
import { useUpdateGroup } from '@/hooks/useGroups'
import type { GroupRead } from '@/types/api'

interface GroupSettingsDialogProps {
  group: GroupRead
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function GroupSettingsDialog({
  group,
  open,
  onOpenChange,
}: GroupSettingsDialogProps) {
  const update = useUpdateGroup(group.id)
  const del = useDeleteGroup()
  const navigate = useNavigate()

  const [name, setName] = useState(group.name)
  const [announcement, setAnnouncement] = useState(group.announcement ?? '')
  const [freeSpeech, setFreeSpeech] = useState(group.free_speech)
  const [allowFreeMention, setAllowFreeMention] = useState(group.allow_agent_free_mention)

  useEffect(() => {
    setName(group.name)
    setAnnouncement(group.announcement ?? '')
    setFreeSpeech(group.free_speech)
    setAllowFreeMention(group.allow_agent_free_mention)
  }, [group])

  const onSave = async () => {
    await update.mutateAsync({
      name,
      announcement: announcement || null,
      free_speech: freeSpeech,
      allow_agent_free_mention: allowFreeMention,
    })
    onOpenChange(false)
  }

  const onDelete = async () => {
    if (
      !confirm(
        `Delete group "${group.name}"? This is a soft-delete; messages and threads stay in the database but the group won't appear in your list anymore.`,
      )
    ) {
      return
    }
    await del.mutateAsync(group.id)
    onOpenChange(false)
    void navigate('/groups')
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Group Settings</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="gs-name">Group name</Label>
            <Input id="gs-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gs-announce">Announcement</Label>
            <Textarea
              id="gs-announce"
              rows={2}
              value={announcement}
              onChange={(e) => setAnnouncement(e.target.value)}
            />
          </div>

          <Separator />

          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Free speech</Label>
              <p className="text-xs text-muted-foreground">
                When enabled, all agents respond freely without needing @mention.
              </p>
            </div>
            <Switch checked={freeSpeech} onCheckedChange={setFreeSpeech} />
          </div>

          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Allow agent free @mention</Label>
              <p className="text-xs text-muted-foreground">
                Allow agents to freely @ any group member in replies.
              </p>
            </div>
            <Switch checked={allowFreeMention} onCheckedChange={setAllowFreeMention} />
          </div>

          <Separator />

          <div className="rounded-lg border border-border bg-muted/30 p-3">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="text-sm font-medium">Members and agents</p>
                <p className="text-xs text-muted-foreground">
                  Add, mute, and remove group participants on the dedicated management page.
                </p>
              </div>
              <Button variant="outline" size="sm" asChild onClick={() => onOpenChange(false)}>
                <Link to={`/groups/${group.id}/members`}>Manage</Link>
              </Button>
            </div>
          </div>
        </div>

        <DialogFooter className="flex sm:justify-between">
          <Button
            variant="outline"
            onClick={onDelete}
            disabled={del.isPending || update.isPending}
            className="text-red-600 hover:bg-red-50 hover:text-red-700"
          >
            {del.isPending ? 'Deleting…' : 'Delete group'}
          </Button>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button onClick={onSave} disabled={update.isPending}>
              {update.isPending ? 'Saving…' : 'Save'}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
