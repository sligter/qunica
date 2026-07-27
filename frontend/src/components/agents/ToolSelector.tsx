import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityMultiSelect } from '@/components/ui/entity-multi-select'
import { cn } from '@/lib/utils'
import type { AgentRead, AgentToolConfig, BuiltinToolRead, ToolPolicy, WorkspaceBackendType } from '@/types/api'

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

export function ToolSelector({
  tools,
  value,
  workspaceBackendType,
  agents = [],
  currentAgentId,
  onChange,
}: ToolSelectorProps) {
  const { t } = useTranslation('agents')
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

  const selectedAssistantIds = useMemo(
    () =>
      (value.assistant_agents ?? [])
        .filter((selection) => selection.enabled)
        .map((selection) => selection.agent_id),
    [value.assistant_agents],
  )

  const setAssistantAgents = (agentIds: string[]) => {
    onChange({
      ...value,
      assistant_agents: agentIds.map((agentId) => ({ agent_id: agentId, enabled: true })),
    })
  }

  const assistantOptions = useMemo(
    () =>
      agents
        .filter((agent) => agent.id !== currentAgentId)
        .map((agent) => ({
          id: agent.id,
          name: agent.name,
          description: agent.description,
          keywords: [agent.id],
          badge: agent.runtime_kind === 'acp' ? t('acpRuntime') : null,
        })),
    [agents, currentAgentId, t],
  )

  return (
    <div className="space-y-3">
      <div className="rounded-md border border-warning bg-warning/55 p-3 text-xs text-warning-foreground">
        {t('tools.notice')}
      </div>
      <div className="space-y-2">
        <p className="text-xs font-medium text-muted-foreground">{t('tools.agentAsTool')}</p>
        <div className="rounded-md border border-border bg-background p-3">
          <p className="text-xs text-muted-foreground">
            {t('tools.agentAsToolDescription')}
          </p>
          <EntityMultiSelect
            className="mt-3"
            id="agent-as-tool"
            items={assistantOptions}
            selectedIds={selectedAssistantIds}
            onChange={setAssistantAgents}
            label={t('tools.agentAsTool')}
            searchPlaceholder={t('tools.searchAgents')}
            emptyText={t('tools.noAgents')}
            namePrefix="@"
          />
        </div>
      </div>
      {POLICY_ORDER.map((policy) => {
        const policyTools = tools.filter((tool) => tool.policy === policy)
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
                      <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
                        {statusText(tool)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">{tool.description}</p>
                    {!executable && checked && (
                      <p className="mt-2 text-[11px] font-medium text-warning-foreground">
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
