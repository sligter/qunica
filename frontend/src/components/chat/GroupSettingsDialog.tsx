import { useEffect, useState } from 'react'

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
import { useUpdateGroup } from '@/hooks/useGroups'
import { cn } from '@/lib/utils'
import type { GroupAgentRead, GroupRead } from '@/types/api'

interface GroupSettingsDialogProps {
  group: GroupRead
  agents: GroupAgentRead[]
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function GroupSettingsDialog({
  group,
  agents,
  open,
  onOpenChange,
}: GroupSettingsDialogProps) {
  const update = useUpdateGroup(group.id)

  const [name, setName] = useState(group.name)
  const [announcement, setAnnouncement] = useState(group.announcement ?? '')
  const [freeSpeech, setFreeSpeech] = useState(group.free_speech)
  const [allowFreeMention, setAllowFreeMention] = useState(group.allow_agent_free_mention)
  const [mutedIds, setMutedIds] = useState<string[]>(group.muted_agent_ids ?? [])
  const [adminIds, setAdminIds] = useState<string[]>(group.admin_agent_ids ?? [])

  useEffect(() => {
    setName(group.name)
    setAnnouncement(group.announcement ?? '')
    setFreeSpeech(group.free_speech)
    setAllowFreeMention(group.allow_agent_free_mention)
    setMutedIds(group.muted_agent_ids ?? [])
    setAdminIds(group.admin_agent_ids ?? [])
  }, [group])

  const toggleMuted = (agentId: string) =>
    setMutedIds((prev) =>
      prev.includes(agentId) ? prev.filter((id) => id !== agentId) : [...prev, agentId],
    )

  const toggleAdmin = (agentId: string) =>
    setAdminIds((prev) =>
      prev.includes(agentId) ? prev.filter((id) => id !== agentId) : [...prev, agentId],
    )

  const onSave = async () => {
    await update.mutateAsync({
      name,
      announcement: announcement || null,
      free_speech: freeSpeech,
      allow_agent_free_mention: allowFreeMention,
      muted_agent_ids: mutedIds,
      admin_agent_ids: adminIds,
    })
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[85vh] overflow-y-auto">
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

          <div className="flex items-center justify-between">
            <div>
              <Label>Free speech</Label>
              <p className="text-xs text-muted-foreground">
                When enabled, all agents respond freely without needing @mention.
              </p>
            </div>
            <Switch checked={freeSpeech} onCheckedChange={setFreeSpeech} />
          </div>

          <div className="flex items-center justify-between">
            <div>
              <Label>Allow agent free @mention</Label>
              <p className="text-xs text-muted-foreground">
                Allow agents to freely @ any group member in replies.
              </p>
            </div>
            <Switch checked={allowFreeMention} onCheckedChange={setAllowFreeMention} />
          </div>

          <Separator />

          {agents.length > 0 && (
            <>
              <div className="space-y-2">
                <Label>Muted agents</Label>
                <p className="text-xs text-muted-foreground">
                  Muted agents will not respond to messages.
                </p>
                <div className="flex flex-wrap gap-2">
                  {agents.map((a) => (
                    <button
                      key={a.agent_id}
                      type="button"
                      onClick={() => toggleMuted(a.agent_id)}
                      className={cn(
                        'rounded-md border px-3 py-1 text-xs transition-colors',
                        mutedIds.includes(a.agent_id)
                          ? 'border-red-400 bg-red-50 text-red-700 dark:bg-red-950 dark:text-red-300'
                          : 'border-border bg-background hover:bg-muted',
                      )}
                    >
                      {a.display_name}
                    </button>
                  ))}
                </div>
              </div>

              <div className="space-y-2">
                <Label>Group administrators (agents)</Label>
                <p className="text-xs text-muted-foreground">
                  Admin agents have elevated permissions in the group.
                </p>
                <div className="flex flex-wrap gap-2">
                  {agents.map((a) => (
                    <button
                      key={a.agent_id}
                      type="button"
                      onClick={() => toggleAdmin(a.agent_id)}
                      className={cn(
                        'rounded-md border px-3 py-1 text-xs transition-colors',
                        adminIds.includes(a.agent_id)
                          ? 'border-primary bg-primary/10 text-primary'
                          : 'border-border bg-background hover:bg-muted',
                      )}
                    >
                      {a.display_name}
                    </button>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={onSave} disabled={update.isPending}>
            {update.isPending ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
