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
import { useClearGroupMessages } from '@/hooks/useGroupMessages'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import type { GroupCommunicationMode, GroupRead } from '@/types/api'

const communicationModeOptions: Array<{
  value: GroupCommunicationMode
  label: string
  description: string
}> = [
  {
    value: 'mesh',
    label: 'Mesh',
    description: 'Peer collaboration for creative or dynamic work.',
  },
  {
    value: 'star',
    label: 'Star',
    description: 'Admin hub agents speak first, then other routed agents.',
  },
  {
    value: 'hierarchical',
    label: 'Hierarchical',
    description: 'Admin agents lead before worker agents.',
  },
  {
    value: 'ring',
    label: 'Ring',
    description: 'Agents take turns in a stable pipeline order.',
  },
]

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
  const clearMessages = useClearGroupMessages(group.id)
  const workspaces = useWorkspaces()
  const navigate = useNavigate()

  const [name, setName] = useState(group.name)
  const [announcement, setAnnouncement] = useState(group.announcement ?? '')
  const [freeSpeech, setFreeSpeech] = useState(group.free_speech)
  const [proactiveMode, setProactiveMode] = useState(group.proactive_mode)
  const [proactiveReplyMultiplier, setProactiveReplyMultiplier] = useState(
    group.proactive_reply_multiplier,
  )
  const [allowFreeMention, setAllowFreeMention] = useState(group.allow_agent_free_mention)
  const [communicationMode, setCommunicationMode] = useState<GroupCommunicationMode>(
    group.communication_mode,
  )

  useEffect(() => {
    setName(group.name)
    setAnnouncement(group.announcement ?? '')
    setFreeSpeech(group.free_speech)
    setProactiveMode(group.proactive_mode)
    setProactiveReplyMultiplier(group.proactive_reply_multiplier)
    setAllowFreeMention(group.allow_agent_free_mention)
    setCommunicationMode(group.communication_mode)
  }, [group])

  const onSave = async () => {
    await update.mutateAsync({
      name,
      announcement: announcement || null,
      free_speech: freeSpeech,
      proactive_mode: proactiveMode,
      proactive_reply_multiplier: proactiveReplyMultiplier,
      allow_agent_free_mention: allowFreeMention,
      communication_mode: communicationMode,
    })
    onOpenChange(false)
  }

  const onClearMessages = async () => {
    if (!confirm('Clear all visible chat records for this group? This cannot be undone.')) {
      return
    }
    await clearMessages.mutateAsync()
  }

  const setSelectedCommunicationMode = (value: string) => {
    if (
      value === 'mesh' ||
      value === 'star' ||
      value === 'hierarchical' ||
      value === 'ring'
    ) {
      setCommunicationMode(value)
    }
  }

  const workspace = workspaces.data?.find((item) => item.id === group.workspace_id)
  const setMinimumProactiveReplyMultiplier = (value: string) => {
    const next = Number.parseInt(value, 10)
    if (Number.isNaN(next)) {
      setProactiveReplyMultiplier(1)
      return
    }
    setProactiveReplyMultiplier(Math.max(1, next))
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
      <DialogContent className="flex max-h-[85vh] w-[95vw] flex-col gap-4 overflow-hidden sm:max-w-2xl">
        <DialogHeader className="shrink-0">
          <DialogTitle>Group Settings</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto pr-1">
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

            <div className="space-y-1.5">
              <Label>Workspace</Label>
              <div className="rounded-md border border-border bg-muted/30 px-3 py-2 text-sm">
                {workspace ? `${workspace.name} (${workspace.backend_type})` : group.workspace_id ?? 'Legacy group without workspace'}
              </div>
              <p className="text-xs text-muted-foreground">
                Each group gets a dedicated workspace auto-created from your group
                workspace root. Group files live under this workspace; rebinding
                is not supported.
              </p>
            </div>

            <Separator />

            <div className="space-y-1.5">
              <Label htmlFor="gs-communication-mode">Communication mode</Label>
              <select
                id="gs-communication-mode"
                value={communicationMode}
                onChange={(event) => setSelectedCommunicationMode(event.target.value)}
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-1 focus:ring-ring"
              >
                {communicationModeOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
              <p className="text-xs text-muted-foreground">
                {
                  communicationModeOptions.find(
                    (option) => option.value === communicationMode,
                  )?.description
                }
              </p>
            </div>

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
                <Label>Proactive mode</Label>
                <p className="text-xs text-muted-foreground">
                  When enabled, agents decide for themselves whether to reply (they may stay silent if they have nothing to add).
                </p>
              </div>
              <Switch checked={proactiveMode} onCheckedChange={setProactiveMode} />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <Label htmlFor="gs-proactive-reply-multiplier">Reply multiplier</Label>
                <p className="text-xs text-muted-foreground">
                  Allows up to routed agents × {proactiveReplyMultiplier} visible replies. Silent
                  turns do not count, and the loop ends early when everyone stays silent.
                </p>
              </div>
              <Input
                id="gs-proactive-reply-multiplier"
                type="number"
                min={1}
                step={1}
                value={proactiveReplyMultiplier}
                onChange={(e) => setMinimumProactiveReplyMultiplier(e.target.value)}
                disabled={!proactiveMode}
                className="w-20"
              />
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

            <div className="space-y-3 rounded-lg border border-border bg-muted/30 p-3">
              {[
                ['Members and agents', 'Add, mute, and remove group participants.', `/groups/${group.id}/members`],
                ['Files', 'Upload and delete group workspace files.', `/groups/${group.id}/files`],
                ['Notes', 'Create and edit shared group notes.', `/groups/${group.id}/notes`],
              ].map(([title, description, href]) => (
                <div key={href} className="flex items-center justify-between gap-3">
                  <div>
                    <p className="text-sm font-medium">{title}</p>
                    <p className="text-xs text-muted-foreground">{description}</p>
                  </div>
                  <Button variant="outline" size="sm" asChild onClick={() => onOpenChange(false)}>
                    <Link to={href}>Manage</Link>
                  </Button>
                </div>
              ))}
            </div>

            <div className="rounded-lg border border-border bg-muted/30 p-3">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="text-sm font-medium">Chat history</p>
                  <p className="text-xs text-muted-foreground">
                    Clear visible chat records for this group.
                  </p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={onClearMessages}
                  disabled={clearMessages.isPending}
                  className="text-red-600 hover:bg-red-50 hover:text-red-700"
                >
                  {clearMessages.isPending ? 'Clearing…' : 'Clear'}
                </Button>
              </div>
            </div>
          </div>
        </div>

        <DialogFooter className="shrink-0 flex sm:justify-between">
          <Button
            variant="outline"
            onClick={onDelete}
            disabled={del.isPending || update.isPending || clearMessages.isPending}
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
