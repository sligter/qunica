import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useDeleteGroup } from '@/hooks/useDeleteGroup'
import { useUpdateGroup } from '@/hooks/useGroups'
import { useClearGroupMessages } from '@/hooks/useGroupMessages'
import { ApiError } from '@/lib/api-v2/client'
import { GroupSchedulerSettingsSection } from '@/pages/group/GroupSchedulerSettingsSection'
import type { GroupCommunicationMode, GroupRead, GroupUpdate } from '@/types/api'

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

interface GroupSettingsTabProps {
  group: GroupRead
}

export function GroupSettingsTab({ group }: GroupSettingsTabProps) {
  const update = useUpdateGroup(group.id)
  const del = useDeleteGroup()
  const clearMessages = useClearGroupMessages(group.id)
  const navigate = useNavigate()

  // Text-like fields: saved via the section Save buttons.
  const [name, setName] = useState(group.name)
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(group.workspace_id ?? '')
  const [announcement, setAnnouncement] = useState(group.announcement ?? '')
  const [proactiveReplyMultiplier, setProactiveReplyMultiplier] = useState(
    group.proactive_reply_multiplier,
  )
  const [freeMentionMaxDispatches, setFreeMentionMaxDispatches] = useState(
    group.agent_free_mention_max_dispatches,
  )

  // Switch/select fields: saved instantly.
  const [freeSpeech, setFreeSpeech] = useState(group.free_speech)
  const [proactiveMode, setProactiveMode] = useState(group.proactive_mode)
  const [allowFreeMention, setAllowFreeMention] = useState(group.allow_agent_free_mention)
  const [communicationMode, setCommunicationMode] = useState<GroupCommunicationMode>(
    group.communication_mode,
  )

  const [basicsError, setBasicsError] = useState<string | null>(null)
  const [commError, setCommError] = useState<string | null>(null)
  const [confirmClearOpen, setConfirmClearOpen] = useState(false)
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false)

  // Sync each field from its own server value. Instant saves (switches/select)
  // invalidate the group query; a whole-object effect would then wipe unsaved
  // text edits in the other sections.
  useEffect(() => {
    setName(group.name)
  }, [group.name])
  useEffect(() => {
    setSelectedWorkspaceId(group.workspace_id ?? '')
  }, [group.workspace_id])
  useEffect(() => {
    setAnnouncement(group.announcement ?? '')
  }, [group.announcement])
  useEffect(() => {
    setFreeSpeech(group.free_speech)
  }, [group.free_speech])
  useEffect(() => {
    setProactiveMode(group.proactive_mode)
  }, [group.proactive_mode])
  useEffect(() => {
    setProactiveReplyMultiplier(group.proactive_reply_multiplier)
  }, [group.proactive_reply_multiplier])
  useEffect(() => {
    setAllowFreeMention(group.allow_agent_free_mention)
  }, [group.allow_agent_free_mention])
  useEffect(() => {
    setFreeMentionMaxDispatches(group.agent_free_mention_max_dispatches)
  }, [group.agent_free_mention_max_dispatches])
  useEffect(() => {
    setCommunicationMode(group.communication_mode)
  }, [group.communication_mode])

  const errorMessage = (err: unknown, fallback: string): string =>
    err instanceof ApiError ? err.message : fallback

  const saveInstant = async (patch: GroupUpdate, revert: () => void) => {
    setCommError(null)
    try {
      await update.mutateAsync(patch)
    } catch (err) {
      revert()
      setCommError(errorMessage(err, 'Failed to update group'))
    }
  }

  const onFreeSpeechChange = (next: boolean) => {
    const previous = freeSpeech
    setFreeSpeech(next)
    void saveInstant({ free_speech: next }, () => setFreeSpeech(previous))
  }

  const onProactiveModeChange = (next: boolean) => {
    const previous = proactiveMode
    setProactiveMode(next)
    void saveInstant({ proactive_mode: next }, () => setProactiveMode(previous))
  }

  const onAllowFreeMentionChange = (next: boolean) => {
    const previous = allowFreeMention
    setAllowFreeMention(next)
    void saveInstant({ allow_agent_free_mention: next }, () =>
      setAllowFreeMention(previous),
    )
  }

  const onCommunicationModeChange = (value: string) => {
    if (
      value !== 'mesh' &&
      value !== 'star' &&
      value !== 'hierarchical' &&
      value !== 'ring'
    ) {
      return
    }
    const previous = communicationMode
    setCommunicationMode(value)
    void saveInstant({ communication_mode: value }, () => setCommunicationMode(previous))
  }

  const basicsDirty =
    name.trim() !== group.name ||
    (selectedWorkspaceId !== '' && selectedWorkspaceId !== (group.workspace_id ?? '')) ||
    announcement !== (group.announcement ?? '')

  const onSaveBasics = async () => {
    setBasicsError(null)
    try {
      await update.mutateAsync({
        name: name.trim(),
        ...(selectedWorkspaceId && selectedWorkspaceId !== group.workspace_id
          ? { workspace_id: selectedWorkspaceId }
          : {}),
        announcement: announcement || null,
      })
    } catch (err) {
      setBasicsError(errorMessage(err, 'Failed to update group'))
    }
  }

  const limitsDirty =
    proactiveReplyMultiplier !== group.proactive_reply_multiplier ||
    freeMentionMaxDispatches !== group.agent_free_mention_max_dispatches

  const onSaveLimits = async () => {
    setCommError(null)
    try {
      await update.mutateAsync({
        proactive_reply_multiplier: proactiveReplyMultiplier,
        agent_free_mention_max_dispatches: freeMentionMaxDispatches,
      })
    } catch (err) {
      setCommError(errorMessage(err, 'Failed to update group'))
    }
  }

  const setMinimumProactiveReplyMultiplier = (value: string) => {
    const next = Number.parseInt(value, 10)
    setProactiveReplyMultiplier(Number.isNaN(next) ? 1 : Math.max(1, next))
  }

  const setMinimumFreeMentionMaxDispatches = (value: string) => {
    const next = Number.parseInt(value, 10)
    setFreeMentionMaxDispatches(Number.isNaN(next) ? 0 : Math.max(0, next))
  }

  return (
    <div className="w-full space-y-10">
      <SettingsSection
        title="Basic information"
        aside={
          <Button
            size="sm"
            onClick={() => void onSaveBasics()}
            disabled={!basicsDirty || update.isPending}
          >
            {update.isPending ? 'Saving…' : 'Save'}
          </Button>
        }
      >
        <SettingsRow label="Group name" htmlFor="gs-name" stacked>
          <Input
            id="gs-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="max-w-xl"
          />
        </SettingsRow>

        <SettingsRow
          label="Announcement"
          description="Shown to agents as shared group context."
          htmlFor="gs-announce"
          stacked
        >
          <Textarea
            id="gs-announce"
            rows={2}
            value={announcement}
            onChange={(e) => setAnnouncement(e.target.value)}
            className="max-w-xl"
          />
        </SettingsRow>

        <SettingsRow
          label="Workspace"
          description="Group files live in this workspace. Choose another existing workspace or create a local workspace to move future group file operations to that folder."
          stacked
        >
          <div className="max-w-xl">
            <WorkspaceField
              value={selectedWorkspaceId}
              onChange={(workspaceId) => {
                if (workspaceId) setSelectedWorkspaceId(workspaceId)
              }}
            />
          </div>
        </SettingsRow>

        {basicsError ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {basicsError}
          </p>
        ) : null}
      </SettingsSection>

      <SettingsSection
        title="Communication"
        aside={
          <Button
            size="sm"
            onClick={() => void onSaveLimits()}
            disabled={!limitsDirty || update.isPending}
          >
            {update.isPending ? 'Saving…' : 'Save'}
          </Button>
        }
      >
        <SettingsRow
          label="Communication mode"
          description={
            communicationModeOptions.find(
              (option) => option.value === communicationMode,
            )?.description
          }
          htmlFor="gs-communication-mode"
        >
          <select
            id="gs-communication-mode"
            value={communicationMode}
            onChange={(event) => onCommunicationModeChange(event.target.value)}
            disabled={update.isPending}
            className="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
          >
            {communicationModeOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </SettingsRow>

        <SettingsRow
          label="Free speech"
          description="When enabled, all agents respond freely without needing @mention."
        >
          <Switch
            checked={freeSpeech}
            onCheckedChange={onFreeSpeechChange}
            disabled={update.isPending}
            aria-label="Free speech"
          />
        </SettingsRow>

        <SettingsRow
          label="Proactive mode"
          description="When enabled, agents decide for themselves whether to reply (they may stay silent if they have nothing to add)."
        >
          <Switch
            checked={proactiveMode}
            onCheckedChange={onProactiveModeChange}
            disabled={update.isPending}
            aria-label="Proactive mode"
          />
        </SettingsRow>

        <SettingsRow
          label="Reply multiplier"
          description={`Allows up to routed agents × ${proactiveReplyMultiplier} visible replies. Silent turns do not count, and the loop ends early when everyone stays silent.`}
          htmlFor="gs-proactive-reply-multiplier"
        >
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
        </SettingsRow>

        <SettingsRow
          label="Allow agent free @mention"
          description="Allow agents to freely @ any group member in replies."
        >
          <Switch
            checked={allowFreeMention}
            onCheckedChange={onAllowFreeMentionChange}
            disabled={update.isPending}
            aria-label="Allow agent free @mention"
          />
        </SettingsRow>

        <SettingsRow
          label="Follow-up limit"
          description={`Allows up to ${freeMentionMaxDispatches} agent-to-agent @mention follow-up turns per send. Set 0 to disable follow-ups.`}
          htmlFor="gs-free-mention-max"
        >
          <Input
            id="gs-free-mention-max"
            type="number"
            min={0}
            step={1}
            value={freeMentionMaxDispatches}
            onChange={(e) => setMinimumFreeMentionMaxDispatches(e.target.value)}
            disabled={!allowFreeMention}
            className="w-20"
          />
        </SettingsRow>

        {commError ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {commError}
          </p>
        ) : null}
      </SettingsSection>

      <GroupSchedulerSettingsSection group={group} />

      <SettingsSection title="Danger">
        <SettingsRow
          label="Chat history"
          description="Clear visible chat records for this group."
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => setConfirmClearOpen(true)}
            disabled={clearMessages.isPending}
            className="border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive"
          >
            {clearMessages.isPending ? 'Clearing…' : 'Clear history'}
          </Button>
        </SettingsRow>

        <SettingsRow
          label="Delete group"
          description="Soft-delete: messages and threads stay in the database, but the group disappears from your list."
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => setConfirmDeleteOpen(true)}
            disabled={del.isPending}
            className="border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive"
          >
            {del.isPending ? 'Deleting…' : 'Delete group'}
          </Button>
        </SettingsRow>
      </SettingsSection>

      <ConfirmDialog
        open={confirmClearOpen}
        onOpenChange={setConfirmClearOpen}
        title="Clear chat history?"
        description="Clear all visible chat records for this group? This cannot be undone."
        confirmLabel="Clear"
        destructive
        onConfirm={async () => {
          await clearMessages.mutateAsync()
        }}
      />

      <ConfirmDialog
        open={confirmDeleteOpen}
        onOpenChange={setConfirmDeleteOpen}
        title={`Delete group "${group.name}"?`}
        description="This is a soft-delete; messages and threads stay in the database but the group won't appear in your list anymore."
        confirmLabel="Delete"
        destructive
        onConfirm={async () => {
          await del.mutateAsync(group.id)
          void navigate('/')
        }}
      />
    </div>
  )
}
