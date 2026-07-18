import { useState } from 'react'
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
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAcpRuntimeVersions, useInstallAcpRuntimeVersion } from '@/hooks/useAcpRuntimeVersions'
import type {
  AcpPermissionPolicy,
  AcpRuntimeChoice,
  AcpRuntimePresetRead,
  AcpRuntimeProfile,
} from '@/types/api'

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
  const [packageSpec, setPackageSpec] = useState('')
  const [installError, setInstallError] = useState<string | null>(null)
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
    } catch (error) {
      setInstallError(error instanceof Error ? error.message : 'Installation failed.')
    }
  }

  return (
    <section className="space-y-3 rounded-md border border-border bg-card p-3">
      <div className="flex items-start gap-2">
        <Terminal className="mt-0.5 h-4 w-4 text-muted-foreground" />
        <div>
          <h3 className="text-sm font-medium">ACP runtime</h3>
          <p className="text-[11px] text-muted-foreground">
            Launches an Agent Client Protocol process for the selected workspace.
          </p>
        </div>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-profile">Runtime preset</Label>
        <select
          id="acp-profile"
          value={selectedProfile}
          onChange={(event) => handlePresetChange(event.target.value)}
          className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <option value="custom">Custom ACP command</option>
          {presets.map((preset) => (
            <option key={preset.id} value={preset.profile}>
              {preset.name}
              {preset.installed ? '' : ' (fallback command)'}
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
                  <span className="block text-[11px] text-muted-foreground">
                    {preset.installed ? 'Local adapter detected' : 'Uses fallback command'}
                  </span>
                </button>
              )
            })}
          </div>
        )}
        {installedPresets.length > 0 && (
          <p className="text-[11px] text-muted-foreground">
            Detected: {installedPresets.map((preset) => preset.name).join(', ')}
          </p>
        )}
        {installedPresets.length === 0 && presets.length > 0 && (
          <p className="text-[11px] text-muted-foreground">
            No local ACP adapter executable was detected. Presets are still selectable
            and will use editable fallback commands.
          </p>
        )}
      </div>

      {missingPresets.length > 0 && (
        <div className="space-y-1 rounded-md border border-dashed border-border p-2">
          {missingPresets.map((preset) => (
            <p key={preset.id} className="flex gap-2 text-[11px] text-muted-foreground">
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
            <span>Version status</span>
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
              <p className="text-xs text-warning-foreground">Unable to check runtime versions.</p>
            ) : (
              <p className="text-xs text-muted-foreground">
                Local: {versionStatus?.local_version ?? 'Not installed'}
                {' · '}
                Remote: {versionStatus?.latest_version ?? 'Unavailable'}
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
                {versionStatus?.installed ? 'Update' : 'Install'}
              </Button>
              <span className="text-xs text-muted-foreground">{versionStatus?.package_name}</span>
            </div>
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                aria-label="Custom package version"
                value={packageSpec}
                onChange={(event) => setPackageSpec(event.target.value)}
                placeholder={`${versionStatus?.package_name ?? selectedPreset.name}@latest`}
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
                Custom install
              </Button>
            </div>
            {installError && <p className="text-xs text-destructive">{installError}</p>}
          </div>
        </details>
      )}

      <div className="grid gap-3 sm:grid-cols-3">
        <RuntimeCapabilityField
          id="acp-model"
          label="Model"
          value={model}
          options={modelOptions}
          placeholder="Adapter default"
          onChange={onModelChange}
          onCommit={onModelCommit}
          onRefresh={onRefreshCapabilities}
          isLoading={capabilitiesLoading}
          stale={capabilitiesStale}
          warning={capabilitiesWarning}
        />
        <RuntimeCapabilityField
          id="acp-mode"
          label="Mode"
          value={mode}
          options={modeOptions}
          placeholder="Adapter default"
          onChange={onModeChange}
        />
        <RuntimeCapabilityField
          id="acp-thinking"
          label="Thinking"
          value={thinkingEffort}
          options={thinkingOptions}
          placeholder="Adapter default"
          onChange={onThinkingEffortChange}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-command">Command</Label>
        <Input
          id="acp-command"
          value={command}
          onChange={(event) => onCommandChange(event.target.value)}
          placeholder="python"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-args">Arguments</Label>
        <Input
          id="acp-args"
          value={argsText}
          onChange={(event) => onArgsTextChange(event.target.value)}
          placeholder="-m my_acp_agent"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="acp-env">Environment</Label>
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
          <Label htmlFor="acp-timeout">Timeout seconds</Label>
          <Input
            id="acp-timeout"
            type="number"
            min={1}
            max={21600}
            value={timeoutSeconds}
            onChange={(event) => onTimeoutSecondsChange(Number(event.target.value) || 3600)}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="acp-permission">Permission requests</Label>
          <select
            id="acp-permission"
            value={permissionPolicy}
            onChange={(event) =>
              onPermissionPolicyChange(event.target.value as AcpPermissionPolicy)
            }
            className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <option value="deny">Deny requests</option>
            <option value="auto_allow">Auto allow</option>
          </select>
        </div>
      </div>
    </section>
  )
}
