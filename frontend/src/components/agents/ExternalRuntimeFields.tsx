import { CircleAlert, Terminal } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
  onProfileChange: (value: AcpRuntimeProfile) => void
  onPresetSelect: (preset: AcpRuntimePresetRead) => void
  onCommandChange: (value: string) => void
  onArgsTextChange: (value: string) => void
  onEnvTextChange: (value: string) => void
  onTimeoutSecondsChange: (value: number) => void
  onPermissionPolicyChange: (value: AcpPermissionPolicy) => void
  onModelChange: (value: string) => void
  onModeChange: (value: string) => void
  onThinkingEffortChange: (value: string) => void
}

function optionsWithCurrentValue(
  options: AcpRuntimeChoice[],
  value: string,
): AcpRuntimeChoice[] {
  if (!value || options.some((option) => option.value === value)) {
    return options
  }
  return [...options, { value, label: value, description: 'Saved custom value' }]
}

interface RuntimeChoiceFieldProps {
  id: string
  label: string
  value: string
  options: AcpRuntimeChoice[]
  placeholder: string
  onChange: (value: string) => void
}

function RuntimeChoiceField({
  id,
  label,
  value,
  options,
  placeholder,
  onChange,
}: RuntimeChoiceFieldProps) {
  const choices = optionsWithCurrentValue(options, value)
  const hasEmptyChoice = choices.some((option) => option.value === '')

  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <select
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        {!hasEmptyChoice && <option value="">{placeholder}</option>}
        {choices.map((option, index) => (
          <option key={`${option.value}-${index}`} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  )
}

function RuntimeTextField({
  id,
  label,
  value,
  placeholder,
  onChange,
}: {
  id: string
  label: string
  value: string
  placeholder: string
  onChange: (value: string) => void
}) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
      />
    </div>
  )
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
  onProfileChange,
  onPresetSelect,
  onCommandChange,
  onArgsTextChange,
  onEnvTextChange,
  onTimeoutSecondsChange,
  onPermissionPolicyChange,
  onModelChange,
  onModeChange,
  onThinkingEffortChange,
}: ExternalRuntimeFieldsProps) {
  const selectedPreset =
    selectedProfile === 'custom'
      ? null
      : presets.find((preset) => preset.profile === selectedProfile) ?? null
  const installedPresets = presets.filter((preset) => preset.installed)
  const missingPresets = presets.filter((preset) => !preset.installed)
  const modelOptions = selectedPreset?.model_options ?? []
  const modeOptions = selectedPreset?.mode_options ?? []
  const thinkingOptions = selectedPreset?.thinking_effort_options ?? []

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
              {preset.installed ? '' : ' (npx fallback)'}
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
                    {preset.installed ? 'Local adapter detected' : 'Uses npx fallback'}
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

      <div className="grid gap-3 sm:grid-cols-3">
        {selectedPreset ? (
          <>
            <RuntimeChoiceField
              id="acp-model"
              label="Model"
              value={model}
              options={modelOptions}
              placeholder="Adapter default"
              onChange={onModelChange}
            />
            <RuntimeChoiceField
              id="acp-mode"
              label="Mode"
              value={mode}
              options={modeOptions}
              placeholder="Adapter default"
              onChange={onModeChange}
            />
            <RuntimeChoiceField
              id="acp-thinking"
              label="Thinking"
              value={thinkingEffort}
              options={thinkingOptions}
              placeholder="Adapter default"
              onChange={onThinkingEffortChange}
            />
          </>
        ) : (
          <>
            <RuntimeTextField
              id="acp-model"
              label="Model"
              value={model}
              placeholder="Default"
              onChange={onModelChange}
            />
            <RuntimeTextField
              id="acp-mode"
              label="Mode"
              value={mode}
              placeholder="Default"
              onChange={onModeChange}
            />
            <RuntimeTextField
              id="acp-thinking"
              label="Thinking"
              value={thinkingEffort}
              placeholder="Default"
              onChange={onThinkingEffortChange}
            />
          </>
        )}
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
