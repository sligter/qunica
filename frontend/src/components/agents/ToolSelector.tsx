import type {
  AgentRead,
  AgentToolConfig,
  BuiltinToolRead,
  McpServerRead,
  ToolPolicy,
  WorkspaceBackendType,
} from '@/types/api'
import { McpToolSelector } from '@/components/agents/McpToolSelector'
import { EntityPicker } from '@/components/ui/entity-picker'
import { Label } from '@/components/ui/label'
import { PageState } from '@/components/ui/page-state'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import { useTranslation } from 'react-i18next'

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
  /** MCP servers the owner has configured, offered alongside the built-ins. */
  mcpServers?: McpServerRead[]
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

export function ToolSelector({
  tools,
  value,
  workspaceBackendType,
  agents = [],
  mcpServers = [],
  currentAgentId,
  onChange,
}: ToolSelectorProps) {
  const { t } = useTranslation(['agents', 'groups'])
  const statusText = (tool: BuiltinToolRead) => {
    if (tool.requires_sandbox && workspaceBackendType === 'local') return t('tools.states.cloudRequired')
    if (isRuntimeExecutable(tool, workspaceBackendType) || tool.runtime_status === 'available') return t('tools.states.executable')
    if (tool.runtime_status === 'planned') return t('tools.states.savedOnly')
    if (tool.runtime_status === 'sandbox_required') return t('tools.states.sandboxRequired')
    return t('tools.states.disabled')
  }
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

  // The picker speaks in plain ids; the stored shape is a list of records, so
  // the mapping lives here rather than leaking that shape into the picker.
  const setAssistantAgents = (agentIds: string[]) => {
    onChange({
      ...value,
      assistant_agents: agentIds.map((agent_id) => ({ agent_id, enabled: true })),
    })
  }

  const selectableAgents = agents.filter((agent) => agent.id !== currentAgentId)
  const bypassApprovals = value.bypass_approvals ?? false

  return (
    <div className="space-y-3">
      <div className="rounded-md border border-warning bg-warning/55 p-3 text-xs text-warning-foreground">
        {t('tools.notice')}
      </div>
      {/* Unattended mode sits with the tools rather than in a general settings
          pane: it only means anything for the agent whose shell it turns loose,
          and the person enabling it is the person picking that shell. */}
      <div
        className={cn(
          'rounded-md border p-3',
          bypassApprovals ? 'border-destructive bg-destructive/10' : 'border-border bg-background',
        )}
      >
        <div className="flex items-start justify-between gap-3">
          <div className="space-y-1">
            <Label htmlFor="tool-bypass-approvals" className="text-sm font-medium">
              {t('tools.bypassApprovals.label')}
            </Label>
            <p className="text-xs text-muted-foreground">{t('tools.bypassApprovals.description')}</p>
            {bypassApprovals && (
              <p className="text-2xs font-medium text-destructive">
                {t('tools.bypassApprovals.warning')}
              </p>
            )}
          </div>
          <Switch
            id="tool-bypass-approvals"
            aria-label={t('tools.bypassApprovals.label')}
            checked={bypassApprovals}
            onCheckedChange={(checked) => onChange({ ...value, bypass_approvals: checked })}
            className="mt-1 shrink-0"
          />
        </div>
      </div>
      <div className="space-y-2">
        <p className="text-xs font-medium text-muted-foreground">{t('tools.agentAsTool')}</p>
        <p className="text-xs text-muted-foreground">{t('groups:delegationDescription')}</p>
        <EntityPicker
          label={t('tools.agentAsTool')}
          searchPlaceholder={t('form.searchAgents')}
          items={selectableAgents.map((agent) => ({
            id: agent.id,
            label: `@${agent.name}`,
            meta: agent.description ?? undefined,
          }))}
          selectedIds={(value.assistant_agents ?? [])
            .filter((selection) => selection.enabled)
            .map((selection) => selection.agent_id)}
          onChange={setAssistantAgents}
          countLabel={(total, selected) =>
            t('form.agentCount', { total, selected, count: total })
          }
          empty={<PageState inset icon={null} title={t('tools.noAgents')} />}
        />
      </div>
      <div className="space-y-2">
        <p className="text-xs font-medium text-muted-foreground">{t('tools.mcp.title')}</p>
        <p className="text-xs text-muted-foreground">{t('tools.mcp.description')}</p>
        <McpToolSelector
          servers={mcpServers}
          value={value.mcp_servers ?? []}
          onChange={(next) => onChange({ ...value, mcp_servers: next })}
        />
      </div>
      {POLICY_ORDER.map((policy) => {
        const policyTools = tools.filter(
          (tool) => tool.policy === policy && tool.runtime_status !== 'planned',
        )
        if (policyTools.length === 0) return null
        return (
          <div key={policy} className="space-y-2">
            <p className="text-xs font-medium text-muted-foreground">{t(`tools.policies.${policy}`)}</p>
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
                      <span className="rounded-full bg-muted px-2 py-0.5 text-2xs text-muted-foreground">
                        {statusText(tool)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">{tool.description}</p>
                    {!executable && checked && (
                      <p className="mt-2 text-2xs font-medium text-warning-foreground">
                        {t('tools.unavailable')}
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
