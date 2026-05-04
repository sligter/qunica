import type { AgentToolConfig, BuiltinToolRead, ToolPolicy, WorkspaceBackendType } from '@/types/api'
import { cn } from '@/lib/utils'

const POLICY_LABELS: Record<ToolPolicy, string> = {
  read: 'Filesystem read',
  write: 'Filesystem write',
  execute: 'Execution',
  network: 'Web',
  media: 'Media',
  planning: 'Planning',
  orchestration: 'Orchestration',
}

const POLICY_ORDER: ToolPolicy[] = [
  'read',
  'write',
  'execute',
  'network',
  'media',
  'planning',
  'orchestration',
]

interface ToolSelectorProps {
  tools: BuiltinToolRead[]
  value: AgentToolConfig
  workspaceBackendType: WorkspaceBackendType
  onChange: (next: AgentToolConfig) => void
}

function isToggleDisabled(tool: BuiltinToolRead, workspaceBackendType: WorkspaceBackendType) {
  return (
    tool.runtime_status === 'disabled' ||
    (tool.requires_sandbox && workspaceBackendType === 'local')
  )
}

function statusText(tool: BuiltinToolRead, workspaceBackendType: WorkspaceBackendType) {
  if (tool.requires_sandbox && workspaceBackendType === 'local') return 'Cloud sandbox required'
  if (tool.runtime_status === 'available') return 'Available'
  if (tool.runtime_status === 'planned') return 'Saved only'
  if (tool.runtime_status === 'sandbox_required') return 'Sandbox required'
  return 'Disabled'
}

export function ToolSelector({
  tools,
  value,
  workspaceBackendType,
  onChange,
}: ToolSelectorProps) {
  const toggleTool = (tool: BuiltinToolRead) => {
    if (isToggleDisabled(tool, workspaceBackendType)) return
    const current = value.tools[tool.id]
    onChange({
      tools: {
        ...value.tools,
        [tool.id]: {
          enabled: !(current?.enabled ?? false),
          policy: current?.policy ?? tool.policy,
        },
      },
    })
  }

  return (
    <div className="space-y-3">
      {POLICY_ORDER.map((policy) => {
        const policyTools = tools.filter((tool) => tool.policy === policy)
        if (policyTools.length === 0) return null
        return (
          <div key={policy} className="space-y-2">
            <p className="text-xs font-medium text-muted-foreground">{POLICY_LABELS[policy]}</p>
            <div className="grid gap-2 sm:grid-cols-2">
              {policyTools.map((tool) => {
                const checked = value.tools[tool.id]?.enabled ?? false
                const disabled = isToggleDisabled(tool, workspaceBackendType)
                return (
                  <button
                    key={tool.id}
                    type="button"
                    disabled={disabled}
                    onClick={() => toggleTool(tool)}
                    className={cn(
                      'rounded-md border p-3 text-left transition-colors',
                      checked
                        ? 'border-primary bg-primary/10 text-foreground'
                        : 'border-border bg-background hover:bg-muted',
                      disabled && 'cursor-not-allowed opacity-50 hover:bg-background',
                    )}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <span className="text-sm font-medium">{tool.name}</span>
                      <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
                        {statusText(tool, workspaceBackendType)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">{tool.description}</p>
                  </button>
                )
              })}
            </div>
          </div>
        )
      })}
    </div>
  )
}
