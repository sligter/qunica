import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

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
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'
import { GroupSchedulerSettingsSection } from '@/pages/group/GroupSchedulerSettingsSection'
import { logTerminalCleanupError } from '@/terminal/logTerminalCleanupError'
import { useTerminalRuntime } from '@/terminal/TerminalRuntimeProvider'
import type { GroupCommunicationMode, GroupRead, GroupUpdate } from '@/types/api'

const communicationModeKeys = {
  mesh: { label: 'settings.modes.mesh', description: 'settings.modes.meshDescription' },
  star: { label: 'settings.modes.star', description: 'settings.modes.starDescription' },
  hierarchical: {
    label: 'settings.modes.hierarchical',
    description: 'settings.modes.hierarchicalDescription',
  },
  ring: { label: 'settings.modes.ring', description: 'settings.modes.ringDescription' },
} as const satisfies Record<GroupCommunicationMode, { label: string; description: string }>

const communicationModes = Object.keys(communicationModeKeys) as GroupCommunicationMode[]

function isCommunicationMode(value: string): value is GroupCommunicationMode {
  return communicationModes.some((mode) => mode === value)
}

interface GroupSettingsTabProps {
  group: GroupRead
}

export function GroupSettingsTab({ group }: GroupSettingsTabProps) {
  const { t, i18n } = useTranslation(['groups', 'common'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const update = useUpdateGroup(group.id)
  const del = useDeleteGroup()
  const clearMessages = useClearGroupMessages(group.id)
  const navigate = useNavigate()
  const { closeConversation } = useTerminalRuntime()

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
  const communicationModeValue = communicationMode as string

  const [basicsError, setBasicsError] = useState<string | null | undefined>(undefined)
  const [commError, setCommError] = useState<string | null | undefined>(undefined)
  const [confirmClearOpen, setConfirmClearOpen] = useState(false)
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false)
  const [clearError, setClearError] = useState<string | undefined>(undefined)
  const [deleteError, setDeleteError] = useState<string | undefined>(undefined)

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

  const errorMessage = (err: unknown): string | null =>
    err instanceof ApiError ? err.message : null
  const rawErrorDetail = (err: unknown): string =>
    err instanceof Error ? err.message : String(err)

  const saveInstant = async (patch: GroupUpdate, revert: () => void) => {
    setCommError(undefined)
    try {
      await update.mutateAsync(patch)
    } catch (err) {
      revert()
      setCommError(errorMessage(err))
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
    setBasicsError(undefined)
    try {
      await update.mutateAsync({
        name: name.trim(),
        ...(selectedWorkspaceId && selectedWorkspaceId !== group.workspace_id
          ? { workspace_id: selectedWorkspaceId }
          : {}),
        announcement: announcement || null,
      })
    } catch (err) {
      setBasicsError(errorMessage(err))
    }
  }

  const limitsDirty =
    proactiveReplyMultiplier !== group.proactive_reply_multiplier ||
    freeMentionMaxDispatches !== group.agent_free_mention_max_dispatches

  const onSaveLimits = async () => {
    setCommError(undefined)
    try {
      await update.mutateAsync({
        proactive_reply_multiplier: proactiveReplyMultiplier,
        agent_free_mention_max_dispatches: freeMentionMaxDispatches,
      })
    } catch (err) {
      setCommError(errorMessage(err))
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
        title={t('settings.basic')}
        description={t('settings.basicDescription')}
        className="max-w-4xl"
        aside={<div className="flex items-center gap-2"><span className="hidden text-xs text-muted-foreground sm:inline">{basicsDirty ? t('settings.unsaved') : t('settings.saved')}</span>
          <Button
            size="sm"
            onClick={() => void onSaveBasics()}
            disabled={!basicsDirty || update.isPending}
          >
            {update.isPending ? t('common:actions.saving') : t('common:actions.save')}
          </Button>
        </div>}
      >
        <SettingsRow label={t('settings.name')} htmlFor="gs-name" stacked className="py-5">
          <Input
            id="gs-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="max-w-2xl"
          />
        </SettingsRow>

        <SettingsRow
          label={t('settings.announcement')}
          description={t('settings.announcementDescription')}
          htmlFor="gs-announce"
          stacked
          className="py-5"
        >
          <Textarea
            id="gs-announce"
            rows={5}
            value={announcement}
            onChange={(e) => setAnnouncement(e.target.value)}
            className="max-w-4xl resize-y"
          />
        </SettingsRow>

        <SettingsRow
          label={t('settings.workspace')}
          description={t('settings.workspaceDescription')}
          stacked
          className="py-5"
        >
          <div className="max-w-3xl">
            <WorkspaceField
              variant="compact"
              value={selectedWorkspaceId}
              onChange={(workspaceId) => {
                if (workspaceId) setSelectedWorkspaceId(workspaceId)
              }}
            />
          </div>
        </SettingsRow>

        {basicsError !== undefined ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {basicsError
              ? t('errors.updateDetail', { message: basicsError })
              : t('errors.update')}
          </p>
        ) : null}
      </SettingsSection>

      <SettingsSection
        title={t('settings.communication')}
        aside={
          <Button
            size="sm"
            onClick={() => void onSaveLimits()}
            disabled={!limitsDirty || update.isPending}
          >
            {update.isPending ? t('common:actions.saving') : t('common:actions.save')}
          </Button>
        }
      >
        <SettingsRow
          label={t('settings.mode')}
          description={
            isCommunicationMode(communicationModeValue)
              ? t(communicationModeKeys[communicationModeValue].description)
              : communicationModeValue
          }
          htmlFor="gs-communication-mode"
        >
          <select
            id="gs-communication-mode"
            value={communicationModeValue}
            onChange={(event) => onCommunicationModeChange(event.target.value)}
            disabled={update.isPending}
            className="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
          >
            {!isCommunicationMode(communicationModeValue) ? (
              <option value={communicationModeValue}>{communicationModeValue}</option>
            ) : null}
            {communicationModes.map((mode) => (
              <option key={mode} value={mode}>
                {t(communicationModeKeys[mode].label)}
              </option>
            ))}
          </select>
        </SettingsRow>

        <SettingsRow
          label={t('settings.freeSpeech')}
          description={t('settings.freeSpeechDescription')}
        >
          <Switch
            checked={freeSpeech}
            onCheckedChange={onFreeSpeechChange}
            disabled={update.isPending}
            aria-label={t('settings.freeSpeech')}
          />
        </SettingsRow>

        <SettingsRow
          label={t('settings.proactive')}
          description={t('settings.proactiveDescription')}
        >
          <Switch
            checked={proactiveMode}
            onCheckedChange={onProactiveModeChange}
            disabled={update.isPending}
            aria-label={t('settings.proactive')}
          />
        </SettingsRow>

        <SettingsRow
          label={t('settings.replyMultiplier')}
          description={t('settings.replyMultiplierDescription', {
            count: proactiveReplyMultiplier,
            formattedCount: formatNumber(proactiveReplyMultiplier, language),
          })}
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
          label={t('settings.freeMention')}
          description={t('settings.freeMentionDescription')}
        >
          <Switch
            checked={allowFreeMention}
            onCheckedChange={onAllowFreeMentionChange}
            disabled={update.isPending}
            aria-label={t('settings.freeMention')}
          />
        </SettingsRow>

        <SettingsRow
          label={t('settings.followUp')}
          description={t('settings.followUpDescription', {
            count: freeMentionMaxDispatches,
            formattedCount: formatNumber(freeMentionMaxDispatches, language),
          })}
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

        {commError !== undefined ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {commError
              ? t('errors.updateDetail', { message: commError })
              : t('errors.update')}
          </p>
        ) : null}
      </SettingsSection>

      <GroupSchedulerSettingsSection group={group} />

      <SettingsSection title={t('settings.danger')}>
        <SettingsRow
          label={t('settings.history')}
          description={t('settings.historyDescription')}
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setClearError(undefined)
              setConfirmClearOpen(true)
            }}
            disabled={clearMessages.isPending}
            className="border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive"
          >
            {clearMessages.isPending ? t('settings.clearing') : t('settings.clearHistory')}
          </Button>
        </SettingsRow>

        {clearError !== undefined ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {t('settings.errors.clearHistory', { message: clearError })}
          </p>
        ) : null}

        <SettingsRow
          label={t('actions.delete')}
          description={t('settings.deleteDescription')}
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setDeleteError(undefined)
              setConfirmDeleteOpen(true)
            }}
            disabled={del.isPending}
            className="border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive"
          >
            {del.isPending ? t('common:actions.deleting') : t('actions.delete')}
          </Button>
        </SettingsRow>

        {deleteError !== undefined ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {t('settings.errors.delete', { message: deleteError })}
          </p>
        ) : null}
      </SettingsSection>

      <ConfirmDialog
        open={confirmClearOpen}
        onOpenChange={setConfirmClearOpen}
        title={t('settings.clearTitle')}
        description={t('settings.clearDescription')}
        confirmLabel={t('common:actions.clear')}
        destructive
        onConfirm={async () => {
          try {
            await clearMessages.mutateAsync()
          } catch (err) {
            const detail = rawErrorDetail(err)
            setClearError(detail)
            setConfirmClearOpen(false)
          }
        }}
      />

      <ConfirmDialog
        open={confirmDeleteOpen}
        onOpenChange={setConfirmDeleteOpen}
        title={t('settings.deleteTitle', { name: group.name })}
        description={t('settings.deleteConfirmDescription')}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          try {
            await del.mutateAsync(group.id)
            await closeConversation(group.id, true).catch(logTerminalCleanupError)
            void navigate('/')
          } catch (err) {
            const detail = rawErrorDetail(err)
            setDeleteError(detail)
            setConfirmDeleteOpen(false)
          }
        }}
      />
    </div>
  )
}
