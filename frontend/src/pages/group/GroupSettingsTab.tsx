import { useEffect, useState } from 'react'
import { ChevronDown, ChevronUp } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { GroupTemplatesSection } from '@/components/groups/GroupTemplatesSection'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useDeleteGroup } from '@/hooks/useDeleteGroup'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useUpdateGroup } from '@/hooks/useGroups'
import { useUnsavedChangesGuard } from '@/hooks/useUnsavedChangesGuard'
import { useClearGroupMessages } from '@/hooks/useGroupMessages'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
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

type ResponseMode = 'mentioned' | 'everyone' | 'proactive'

const responseModeKeys = {
  mentioned: {
    label: 'settings.responseModes.mentioned',
    description: 'settings.responseModes.mentionedDescription',
  },
  everyone: {
    label: 'settings.responseModes.everyone',
    description: 'settings.responseModes.everyoneDescription',
  },
  proactive: {
    label: 'settings.responseModes.proactive',
    description: 'settings.responseModes.proactiveDescription',
  },
} as const satisfies Record<ResponseMode, { label: string; description: string }>

const responseModes = Object.keys(responseModeKeys) as ResponseMode[]

function isCommunicationMode(value: string): value is GroupCommunicationMode {
  return communicationModes.some((mode) => mode === value)
}

interface GroupSettingsTabProps {
  group: GroupRead
  compact?: boolean
}

export function GroupSettingsTab({ group, compact = false }: GroupSettingsTabProps) {
  const { t } = useTranslation(['groups', 'common'])
  const update = useUpdateGroup(group.id)
  const groupAgents = useGroupAgents(group.id)
  const del = useDeleteGroup()
  const clearMessages = useClearGroupMessages(group.id)
  const navigate = useNavigate()
  const { closeConversation } = useTerminalRuntime()

  // Text-like fields: saved via the section Save buttons.
  const [name, setName] = useState(group.name)
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(group.workspace_id ?? '')
  const [announcement, setAnnouncement] = useState(group.announcement ?? '')

  // Switch/select fields: saved instantly.
  const [freeSpeech, setFreeSpeech] = useState(group.free_speech)
  const [proactiveMode, setProactiveMode] = useState(group.proactive_mode)
  const [autoShareWorkspace, setAutoShareWorkspace] = useState(
    group.auto_share_workspace_with_new_agents ?? true,
  )
  const [communicationMode, setCommunicationMode] = useState<GroupCommunicationMode>(
    group.communication_mode,
  )
  const [defaultSpeakingOrder, setDefaultSpeakingOrder] = useState<string[] | null>(
    group.default_speaking_order ?? null,
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
    setAutoShareWorkspace(group.auto_share_workspace_with_new_agents ?? true)
  }, [group.auto_share_workspace_with_new_agents])
  useEffect(() => {
    setCommunicationMode(group.communication_mode)
  }, [group.communication_mode])
  useEffect(() => {
    setDefaultSpeakingOrder(group.default_speaking_order ?? null)
  }, [group.default_speaking_order])

  const agentsById = new Map(
    (groupAgents.data ?? []).map((agent) => [agent.agent_id, agent]),
  )
  const configuredAgents = (defaultSpeakingOrder ?? []).flatMap((agentId) => {
    const agent = agentsById.get(agentId)
    return agent ? [agent] : []
  })
  const configuredIds = new Set(configuredAgents.map((agent) => agent.agent_id))
  const orderedAgents = [
    ...configuredAgents,
    ...(groupAgents.data ?? []).filter((agent) => !configuredIds.has(agent.agent_id)),
  ]

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

  const responseMode: ResponseMode = proactiveMode
    ? 'proactive'
    : freeSpeech
      ? 'everyone'
      : 'mentioned'

  const onResponseModeChange = (value: string) => {
    if (!responseModes.includes(value as ResponseMode)) return
    const previousFreeSpeech = freeSpeech
    const previousProactiveMode = proactiveMode
    const nextFreeSpeech = value === 'everyone'
    const nextProactiveMode = value === 'proactive'
    setFreeSpeech(nextFreeSpeech)
    setProactiveMode(nextProactiveMode)
    void saveInstant(
      { free_speech: nextFreeSpeech, proactive_mode: nextProactiveMode },
      () => {
        setFreeSpeech(previousFreeSpeech)
        setProactiveMode(previousProactiveMode)
      },
    )
  }

  const onAutoShareWorkspaceChange = (next: boolean) => {
    const previous = autoShareWorkspace
    setAutoShareWorkspace(next)
    void saveInstant({ auto_share_workspace_with_new_agents: next }, () =>
      setAutoShareWorkspace(previous),
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

  const onDefaultSpeakingOrderChange = (enabled: boolean) => {
    const previous = defaultSpeakingOrder
    const next = enabled ? orderedAgents.map((agent) => agent.agent_id) : null
    setDefaultSpeakingOrder(next)
    void saveInstant({ default_speaking_order: next }, () =>
      setDefaultSpeakingOrder(previous),
    )
  }

  const moveDefaultSpeaker = (index: number, offset: -1 | 1) => {
    const target = index + offset
    if (target < 0 || target >= orderedAgents.length) return
    const previous = defaultSpeakingOrder
    const next = orderedAgents.map((agent) => agent.agent_id)
    const current = next[index]
    next[index] = next[target]
    next[target] = current
    setDefaultSpeakingOrder(next)
    void saveInstant({ default_speaking_order: next }, () =>
      setDefaultSpeakingOrder(previous),
    )
  }

  const basicsDirty =
    name.trim() !== group.name ||
    (selectedWorkspaceId !== '' && selectedWorkspaceId !== (group.workspace_id ?? '')) ||
    announcement !== (group.announcement ?? '')
  useUnsavedChangesGuard(basicsDirty)

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

  return (
    <div
      className={cn(
        'w-full',
        compact ? 'group-drawer-settings space-y-4' : 'space-y-10',
      )}
    >
      <SettingsSection
        title={t('settings.basic')}
        description={t('settings.basicDescription')}
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
            className="w-full"
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
            className="w-full resize-y"
          />
        </SettingsRow>

        <SettingsRow
          label={t('settings.workspace')}
          description={t('settings.workspaceDescription')}
          stacked
          className="py-5"
        >
          <div className="w-full">
            <WorkspaceField
              variant="compact"
              value={selectedWorkspaceId}
              onChange={(workspaceId) => {
                if (workspaceId) setSelectedWorkspaceId(workspaceId)
              }}
            />
          </div>
        </SettingsRow>

        <SettingsRow
          label={t('settings.autoShareWorkspace')}
          description={t('settings.autoShareWorkspaceDescription')}
        >
          <Switch
            checked={autoShareWorkspace}
            onCheckedChange={onAutoShareWorkspaceChange}
            disabled={update.isPending}
            aria-label={t('settings.autoShareWorkspace')}
          />
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
        description={t('settings.communicationDescription')}
      >
        <SettingsRow
          label={t('settings.responseMode')}
          description={t(responseModeKeys[responseMode].description)}
          htmlFor="gs-response-mode"
        >
          <select
            id="gs-response-mode"
            value={responseMode}
            onChange={(event) => onResponseModeChange(event.target.value)}
            disabled={update.isPending}
            className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
          >
            {responseModes.map((mode) => (
              <option key={mode} value={mode}>
                {t(responseModeKeys[mode].label)}
              </option>
            ))}
          </select>
        </SettingsRow>

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
            className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
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
          label={t('settings.defaultSpeakingOrder')}
          description={t('settings.defaultSpeakingOrderDescription')}
        >
          <Switch
            checked={defaultSpeakingOrder !== null}
            onCheckedChange={onDefaultSpeakingOrderChange}
            disabled={update.isPending || groupAgents.isLoading}
            aria-label={t('settings.defaultSpeakingOrder')}
          />
        </SettingsRow>

        {defaultSpeakingOrder !== null ? (
          <div className="py-2.5">
            {orderedAgents.length > 0 ? (
              <ol
                aria-label={t('settings.defaultSpeakingOrder')}
                className="max-h-64 divide-y divide-border overflow-y-auto rounded-md border border-border"
              >
                {orderedAgents.map((agent, index) => (
                  <li key={agent.agent_id} className="flex items-center gap-3 px-3 py-2">
                    <span className="w-5 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
                      {index + 1}
                    </span>
                    <AgentAvatar
                      name={agent.display_name}
                      avatarUrl={agent.avatar_url}
                      size="sm"
                    />
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                      {agent.display_name}
                    </span>
                    <div className="flex shrink-0 items-center gap-1">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        aria-label={t('settings.moveSpeakerEarlier', { name: agent.display_name })}
                        disabled={index === 0 || update.isPending}
                        onClick={() => moveDefaultSpeaker(index, -1)}
                      >
                        <ChevronUp aria-hidden className="h-4 w-4" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        aria-label={t('settings.moveSpeakerLater', { name: agent.display_name })}
                        disabled={index === orderedAgents.length - 1 || update.isPending}
                        onClick={() => moveDefaultSpeaker(index, 1)}
                      >
                        <ChevronDown aria-hidden className="h-4 w-4" />
                      </Button>
                    </div>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="rounded-md border border-dashed border-border px-3 py-4 text-center text-sm text-muted-foreground">
                {t('settings.noAgentsForSpeakingOrder')}
              </p>
            )}
          </div>
        ) : null}

        <p className="py-2 text-xs text-muted-foreground">
          {t('delegationDescription')}
        </p>

        {commError !== undefined ? (
          <p className="py-2 text-sm text-destructive" role="alert">
            {commError
              ? t('errors.updateDetail', { message: commError })
              : t('errors.update')}
          </p>
        ) : null}
      </SettingsSection>

      <details className="group w-full border-y border-border">
        <summary className="flex cursor-pointer list-none items-center justify-between gap-4 rounded-sm py-3 outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
          <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            {t('common:advancedScheduling')}
          </span>
          <ChevronDown
            aria-hidden
            className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-180"
          />
        </summary>
        <div className="pb-2 pt-6">
          <GroupSchedulerSettingsSection group={group} />
        </div>
      </details>

      <GroupTemplatesSection group={group} />

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
