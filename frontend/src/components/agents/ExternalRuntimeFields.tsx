import { AlertTriangle, CheckCircle2, Terminal, XCircle } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useExternalRuntimes } from '@/hooks/useExternalRuntimes'
import { cn } from '@/lib/utils'
import type { ExternalRuntimeAdapter } from '@/types/api'

interface ExternalRuntimeFieldsProps {
  adapter: ExternalRuntimeAdapter
  executable: string
  timeoutSeconds: number
  maxTurns: number
  onAdapterChange: (adapter: ExternalRuntimeAdapter) => void
  onExecutableChange: (value: string) => void
  onTimeoutSecondsChange: (value: number) => void
  onMaxTurnsChange: (value: number) => void
}

const ADAPTER_OPTIONS: { value: ExternalRuntimeAdapter; label: string; command: string }[] = [
  { value: 'codex', label: 'Codex CLI', command: 'codex exec' },
  { value: 'claude_code', label: 'Claude Code', command: 'claude -p' },
]

export function ExternalRuntimeFields({
  adapter,
  executable,
  timeoutSeconds,
  maxTurns,
  onAdapterChange,
  onExecutableChange,
  onTimeoutSecondsChange,
  onMaxTurnsChange,
}: ExternalRuntimeFieldsProps) {
  const status = useExternalRuntimes()
  const selectedStatus = status.data?.adapters.find((item) => item.adapter === adapter)

  return (
    <section className="space-y-3 rounded-md border border-border bg-card p-3">
      <div className="flex items-start gap-2">
        <Terminal className="mt-0.5 h-4 w-4 text-muted-foreground" />
        <div>
          <h3 className="text-sm font-medium">External CLI runtime</h3>
          <p className="text-[11px] text-muted-foreground">
            Runs in the selected workspace with full-auto CLI permissions.
          </p>
        </div>
      </div>

      <div className="grid gap-2 sm:grid-cols-2">
        {ADAPTER_OPTIONS.map((option) => {
          const checked = adapter === option.value
          return (
            <button
              key={option.value}
              type="button"
              onClick={() => onAdapterChange(option.value)}
              className={cn(
                'rounded-md border px-3 py-2 text-left transition-colors',
                checked ? 'border-primary bg-primary/10' : 'border-border bg-background hover:bg-muted',
              )}
            >
              <span className="block text-sm font-medium">{option.label}</span>
              <span className="block text-[11px] text-muted-foreground">{option.command}</span>
            </button>
          )
        })}
      </div>

      <div className="rounded-md border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900">
        <div className="flex gap-2">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          <p>
            This runtime can edit files and run commands through the selected CLI. Use a workspace
            dedicated to the task.
          </p>
        </div>
      </div>

      {selectedStatus && (
        <div className="flex items-start gap-2 rounded-md border border-border bg-background p-3 text-xs">
          {selectedStatus.available ? (
            <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-emerald-600" />
          ) : (
            <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-red-600" />
          )}
          <div>
            <p className="font-medium">
              {selectedStatus.available ? 'CLI detected' : 'CLI not detected'}
            </p>
            <p className="text-muted-foreground">
              {selectedStatus.resolved_path ?? selectedStatus.error ?? selectedStatus.executable}
            </p>
            {selectedStatus.version && (
              <p className="text-muted-foreground">{selectedStatus.version}</p>
            )}
          </div>
        </div>
      )}

      <div className="space-y-1.5">
        <Label htmlFor="external-executable">Executable override (optional)</Label>
        <Input
          id="external-executable"
          value={executable}
          onChange={(event) => onExecutableChange(event.target.value)}
          placeholder={adapter === 'codex' ? 'codex' : 'claude'}
        />
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="external-timeout">Timeout seconds</Label>
          <Input
            id="external-timeout"
            type="number"
            min={1}
            max={21600}
            value={timeoutSeconds}
            onChange={(event) => onTimeoutSecondsChange(Number(event.target.value) || 3600)}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="external-max-turns">Max turns</Label>
          <Input
            id="external-max-turns"
            type="number"
            min={1}
            max={100}
            value={maxTurns}
            onChange={(event) => onMaxTurnsChange(Number(event.target.value) || 20)}
            disabled={adapter !== 'claude_code'}
          />
        </div>
      </div>
    </section>
  )
}
