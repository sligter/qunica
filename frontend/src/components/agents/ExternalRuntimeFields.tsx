import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  CircleAlert,
  CircleCheck,
  Download,
  LoaderCircle,
  PackagePlus,
  Terminal,
  TriangleAlert,
} from 'lucide-react'

import { RuntimeCapabilityField } from '@/components/agents/RuntimeCapabilityField'
import { DEFAULT_ACP_TIMEOUT_SECONDS } from '@/components/agents/acpRuntimeConfig'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Panel } from '@/components/ui/panel'
import { useAcpRuntimeVersions, useInstallAcpRuntimeVersion } from '@/hooks/useAcpRuntimeVersions'
import type {
  AcpPermissionPolicy,
  AcpRuntimeChoice,
  AcpRuntimePresetRead,
  AcpRuntimeProfile,
} from '@/types/api'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'

interface ExternalRuntimeFieldsProps {
  presets?: AcpRuntimePresetRead[]
  selectedProfile: AcpRuntimeProfile
  command: string
  argsText: string
  envText: string
  timeoutSeconds: number
  permissionPolicy: AcpPermissionPolicy
  model: string
  mode: string
  thinkingEffort: string
  modelOptions?: AcpRuntimeChoice[]
  modeOptions?: AcpRuntimeChoice[]
  thinkingEffortOptions?: AcpRuntimeChoice[]
  capabilitiesLoading?: boolean
  capabilitiesStale?: boolean
  capabilitiesWarning?: string | null
  onProfileChange: (value: AcpRuntimeProfile) => void
  onPresetSelect: (preset: AcpRuntimePresetRead) => void
  onCommandChange: (value: string) => void
  onArgsTextChange: (value: string) => void
  onEnvTextChange: (value: string) => void
  onTimeoutSecondsChange: (value: number) => void
  onPermissionPolicyChange: (value: AcpPermissionPolicy) => void
  onModelChange: (value: string) => void
  onModelCommit: (value: string) => void
  onModeChange: (value: string) => void
  onThinkingEffortChange: (value: string) => void
  onRefreshCapabilities: () => void
}

export function ExternalRuntimeFields({
  presets = [],
  selectedProfile,
  command,
  argsText,
  envText,
  timeoutSeconds,
  permissionPolicy,
  model,
  mode,
  thinkingEffort,
  modelOptions: discoveredModelOptions = [],
  modeOptions: discoveredModeOptions = [],
  thinkingEffortOptions: discoveredThinkingOptions = [],
  capabilitiesLoading = false,
  capabilitiesStale = false,
  capabilitiesWarning = null,
  onProfileChange,
  onPresetSelect,
  onCommandChange,
  onArgsTextChange,
  onEnvTextChange,
  onTimeoutSecondsChange,
  onPermissionPolicyChange,
  onModelChange,
  onModelCommit,
  onModeChange,
  onThinkingEffortChange,
  onRefreshCapabilities,
}: ExternalRuntimeFieldsProps) {
  const { t } = useTranslation(['agents', 'common'])
  const [packageSpec, setPackageSpec] = useState('')
  const [installError, setInstallError] = useState<LocalizedError | null>(null)
  const runtimeVersions = useAcpRuntimeVersions()
  const installRuntime = useInstallAcpRuntimeVersion()
  const selectedPreset =
    selectedProfile === 'custom'
      ? null
      : presets.find((preset) => preset.profile === selectedProfile) ?? null
  const installedPresets = presets.filter((preset) => preset.installed)
  const missingPresets = presets.filter((preset) => !preset.installed)
  const modelOptions =
    discoveredModelOptions.length > 0
      ? discoveredModelOptions
      : (selectedPreset?.model_options ?? [])
  const modeOptions =
    discoveredModeOptions.length > 0
      ? discoveredModeOptions
      : (selectedPreset?.mode_options ?? [])
  const thinkingOptions =
    discoveredThinkingOptions.length > 0
      ? discoveredThinkingOptions
      : (selectedPreset?.thinking_effort_options ?? [])
  const versionStatus = selectedPreset
    ? runtimeVersions.data?.presets.find((status) => status.id === selectedPreset.id) ?? null
    : null
  const installPendingLabel = versionStatus?.installed
    ? t('agents:runtime.updating')
    : t('agents:runtime.installing')

  const handlePresetChange = (value: string) => {
    if (value === 'custom') {
      onProfileChange('custom')
      return
    }
    const preset = presets.find((item) => item.profile === value)
    if (preset) {
      onPresetSelect(preset)
    }
  }

  const install = async (custom = false) => {
    if (!selectedPreset) return
    setInstallError(null)
    try {
      await installRuntime.mutateAsync({
        presetId: selectedPreset.id,
        packageSpec: custom ? packageSpec.trim() : undefined,
      })
      setPackageSpec('')
      onRefreshCapabilities()
    } catch (error) {
      setInstallError(error instanceof Error ? messageError(error.message) : translatedError('agents:runtime.installationFailed'))
    }
  }

  return (
    <Panel
      variant="inset"
      icon={Terminal}
      title={t('agents:runtime.title')}
      description={t('agents:runtime.description')}
      contentClassName="space-y-3"
    >
      <div className="space-y-1.5">
        <Label htmlFor="acp-profile">{t('agents:runtime.preset')}</Label>
        <select
          id="acp-profile"
          value={selectedProfile}
          onChange={(event) => handlePresetChange(event.target.value)}
          className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <option value="custom">{t('agents:runtime.customCommand')}</option>
          {presets.map((preset) => (
            <option key={preset.id} value={preset.profile}>
              {preset.name}
              {preset.installed ? '' : t('agents:runtime.fallbackSuffix')}
            </option>
          ))}
        </select>
        {presets.length > 0 && (
          <div className="grid gap-2 sm:grid-cols-2">
            {presets.map((preset) => {
              const selected = selectedProfile === preset.profile
              return (
                <button
                  key={preset.id}
                  type="button"
                  onClick={() => onPresetSelect(preset)}
                  className={[
                    'rounded-md border px-3 py-2 text-left text-sm transition-colors',
                    selected
                      ? 'border-primary bg-primary/10 text-foreground'
                      : 'border-border bg-background hover:bg-muted',
                  ].join(' ')}
                >
                  <span className="block font-medium">{preset.name}</span>
                  <span className="block text-2xs text-muted-foreground">
                    {preset.installed ? t('agents:runtime.localDetected') : t('agents:runtime.usesFallback')}
                  </span>
                </button>
              )
            })}
          </div>
        )}
        {installedPresets.length > 0 && (
          <p className="text-2xs text-muted-foreground">
            {t('agents:runtime.detected', { names: installedPresets.map((preset) => preset.name).join(', ') })}
          </p>
        )}
        {installedPresets.length === 0 && presets.length > 0 && (
          <p className="text-2xs text-muted-foreground">
            {t('agents:runtime.noneDetected')}
          </p>
        )}
        {/* Presets differ in what they can actually stream — dsh, for one, has
            no tool updates and no resumable sessions. Say so here rather than
            letting the empty tool timeline speak for itself. */}
        {selectedPreset?.description && (
          <p className="text-2xs text-muted-foreground">{selectedPreset.description}</p>
        )}
      </div>

      {missingPresets.length > 0 && (
        <div className="space-y-1 rounded-md border border-dashed border-border p-2">
          {missingPresets.map((preset) => (
            <p key={preset.id} className="flex gap-2 text-2xs text-muted-foreground">
              <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>
                {preset.name}: {preset.install_hint}
              </span>
            </p>
          ))}
        </div>
      )}

      {selectedPreset && (
        <details className="rounded-md border border-border bg-background" open>
          <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-3 py-2 text-sm font-medium marker:content-none">
            <span>{t('agents:runtime.versionStatus')}</span>
            {runtimeVersions.isFetching ? (
              <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" />
            ) : versionStatus?.status === 'current' ? (
              <CircleCheck className="h-4 w-4 text-success" />
            ) : (
              <TriangleAlert className="h-4 w-4 text-warning-foreground" />
            )}
          </summary>
          <div className="space-y-3 border-t border-border px-3 py-3">
            {runtimeVersions.isError ? (
              <p className="text-xs text-warning-foreground">{t('agents:runtime.versionError')}</p>
            ) : (
              <p className="text-xs text-muted-foreground">
                {t('agents:runtime.localVersion', { version: versionStatus?.local_version ?? t('agents:runtime.notInstalled') })}
                {' · '}
                {t('agents:runtime.remoteVersion', { version: versionStatus?.latest_version ?? t('agents:runtime.unavailable') })}
              </p>
            )}
            {versionStatus?.message && (
              <p className="text-xs text-warning-foreground">{versionStatus.message}</p>
            )}
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                size="sm"
                onClick={() => void install()}
                disabled={installRuntime.isPending}
              >
                {installRuntime.isPending ? (
                  <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                {installRuntime.isPending
                  ? installPendingLabel
                  : versionStatus?.installed
                    ? t('common:actions.update')
                    : t('common:actions.install')}
              </Button>
              <span className="text-xs text-muted-foreground">{versionStatus?.package_name}</span>
            </div>
            {installRuntime.isPending && (
              <progress
                aria-label={installPendingLabel}
                className="h-1.5 w-full accent-primary"
              />
            )}
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                aria-label={t('agents:runtime.customPackageVersion')}
                value={packageSpec}
                onChange={(event) => setPackageSpec(event.target.value)}
                placeholder={
                  versionStatus?.default_package_spec ??
                  `${versionStatus?.package_name ?? selectedPreset.name}@latest`
                }
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="shrink-0"
                onClick={() => void install(true)}
                disabled={installRuntime.isPending || packageSpec.trim() === ''}
              >
                <PackagePlus className="h-3.5 w-3.5" />
                {t('agents:runtime.customInstall')}
              </Button>
            </div>
            {localizedErrorText(installError, t) && <p className="text-xs text-destructive">{localizedErrorText(installError, t)}</p>}
          </div>
        </details>
      )}

      <div className="grid gap-3 sm:grid-cols-3">
        <RuntimeCapabilityField
          id="acp-model"
          label={t('agents:fields.model')}
          value={model}
          options={modelOptions}
          placeholder={t('agents:runtime.adapterDefault')}
          onChange={onModelChange}
          onCommit={onModelCommit}
          onRefresh={onRefreshCapabilities}
          isLoading={capabilitiesLoading}
          stale={capabilitiesStale}
          warning={capabilitiesWarning}
        />
        <RuntimeCapabilityField
          id="acp-mode"
          label={t('agents:fields.mode')}
          value={mode}
          options={modeOptions}
          placeholder={t('agents:runtime.adapterDefault')}
          onChange={onModeChange}
        />
        <RuntimeCapabilityField
          id="acp-thinking"
          label={t('agents:fields.thinking')}
          value={thinkingEffort}
          options={thinkingOptions}
          placeholder={t('agents:runtime.adapterDefault')}
          onChange={onThinkingEffortChange}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-command">{t('agents:runtime.command')}</Label>
        <Input
          id="acp-command"
          value={command}
          onChange={(event) => onCommandChange(event.target.value)}
          placeholder="python"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-args">{t('agents:runtime.arguments')}</Label>
        <Input
          id="acp-args"
          value={argsText}
          onChange={(event) => onArgsTextChange(event.target.value)}
          placeholder="-m my_acp_agent"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-env">{t('agents:runtime.environment')}</Label>
        <textarea
          id="acp-env"
          value={envText}
          onChange={(event) => onEnvTextChange(event.target.value)}
          rows={3}
          placeholder="KEY=value"
          className="flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="acp-timeout">{t('agents:runtime.timeout')}</Label>
          <Input
            id="acp-timeout"
            type="number"
            min={1}
            value={timeoutSeconds}
            onChange={(event) => onTimeoutSecondsChange(Number(event.target.value) || DEFAULT_ACP_TIMEOUT_SECONDS)}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="acp-permission">{t('agents:runtime.permissions')}</Label>
          <select
            id="acp-permission"
            value={permissionPolicy}
            onChange={(event) =>
              onPermissionPolicyChange(event.target.value as AcpPermissionPolicy)
            }
            className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <option value="deny">{t('agents:runtime.denyRequests')}</option>
            <option value="auto_allow">{t('agents:runtime.autoAllow')}</option>
          </select>
        </div>
      </div>
    </Panel>
  )
}
