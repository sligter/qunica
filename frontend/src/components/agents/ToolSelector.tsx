import type { AgentRead, AgentToolConfig, BuiltinToolRead, ToolPolicy, WorkspaceBackendType } from '@/types/api'
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
  agents?: AgentRead[]
  currentAgentId?: string
  onChange: (next: AgentToolConfig) => void
}

function isRuntimeExecutable(tool: BuiltinToolRead, workspaceBackendType: WorkspaceBackendType) {
  return tool.runtime_status === 'available' && !(tool.requires_sandbox && workspaceBackendType === 'local')
}

function isToggleDisabled(tool: BuiltinToolRead, workspaceBackendType: WorkspaceBackendType) {
  return (
    tool.runtime_status === 'disabled' ||
    (tool.requires_sandbox && workspaceBackendType === 'local')
  )
}

function statusText(tool: BuiltinToolRead, workspaceBackendType: WorkspaceBackendType) {
  if (tool.requires_sandbox && workspaceBackendType === 'local') return 'Cloud sandbox required'
  if (isRuntimeExecutable(tool, workspaceBackendType)) return 'Executable now'
  if (tool.runtime_status === 'available') return 'Executable now'
  if (tool.runtime_status === 'planned') return 'Saved only'
  if (tool.runtime_status === 'sandbox_required') return 'Sandbox required'
  return 'Disabled'
}

export function ToolSelector({
  tools,
  value,
  workspaceBackendType,
  agents = [],
  currentAgentId,
  onChange,
}: ToolSelectorProps) {
  const toggleTool = (tool: BuiltinToolRead) => {
    if (isToggleDisabled(tool, workspaceBackendType)) return
    const current = value.tools[tool.id]
    onChange({
      ...value,
      tools: {
        ...value.tools,
        [tool.id]: {
          enabled: !(current?.enabled ?? false),
          policy: current?.policy ?? tool.policy,
        },
      },
    })
  }

  const toggleAssistantAgent = (agentId: string) => {
    const current = value.assistant_agents ?? []
    const exists = current.some((selection) => selection.agent_id === agentId && selection.enabled)
    onChange({
      ...value,
      assistant_agents: exists
        ? current.filter((selection) => selection.agent_id !== agentId)
        : [...current.filter((selection) => selection.agent_id !== agentId), { agent_id: agentId, enabled: true }],
    })
  }

  const selectableAgents = agents.filter((agent) => agent.id !== currentAgentId)

  return (
    <div className="space-y-3">
      <div className="rounded-md border border-warning bg-warning/55 p-3 text-xs text-warning-foreground">
        Selected built-ins are bound as executable provider-native tools with bounded safeguards.
        Some tools may return setup-required or input-required results when a provider or resume
        contract is not configured; they are still truthful runtime tool calls, not saved-only claims.
      </div>
      <div className="space-y-2">
        <p className="text-xs font-medium text-muted-foreground">Agent as tool</p>
        <div className="rounded-md border border-border bg-background p-3">
          <p className="text-xs text-muted-foreground">
            Bind other owned agents as callable assistants. Runtime delegation dispatches a bounded
            @mention-style task to the selected helper and returns the helper response as a tool result.
          </p>
          {selectableAgents.length === 0 ? (
            <p className="mt-2 text-[11px] text-muted-foreground">No other agents are available.</p>
          ) : (
            <div className="mt-3 flex flex-wrap gap-2">
              {selectableAgents.map((agent) => {
                const checked = (value.assistant_agents ?? []).some(
                  (selection) => selection.agent_id === agent.id && selection.enabled,
                )
                return (
                  <button
                    key={agent.id}
                    type="button"
                    onClick={() => toggleAssistantAgent(agent.id)}
                    className={cn(
                      'rounded-md border px-3 py-2 text-left text-xs transition-colors',
                      checked
                        ? 'border-primary bg-primary text-primary-foreground'
                        : 'border-border bg-background hover:bg-muted',
                    )}
                  >
                    <span className="block font-medium">@{agent.name}</span>
                    {agent.description && (
                      <span className="block max-w-48 truncate opacity-75">{agent.description}</span>
                    )}
                  </button>
                )
              })}
            </div>
          )}
        </div>
      </div>
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
                const executable = isRuntimeExecutable(tool, workspaceBackendType)
                return (
                  <button
                    key={tool.id}
                    type="button"
                    disabled={disabled}
                    onClick={() => toggleTool(tool)}
                    className={cn(
                      'rounded-md border p-3 text-left transition-colors',
                      checked && executable && 'border-primary bg-primary/10 text-foreground',
                      checked && !executable && 'border-warning-foreground/50 bg-warning/55 text-foreground',
                      !checked && 'border-border bg-background hover:bg-muted',
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
                    {!executable && checked && (
                      <p className="mt-2 text-[11px] font-medium text-warning-foreground">
                        This tool is unavailable for the selected workspace backend or disabled by policy.
                      </p>
                    )}
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
