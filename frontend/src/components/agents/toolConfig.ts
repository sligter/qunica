import type { AgentAssistantToolSelection, AgentToolConfig, BuiltinToolRead } from '@/types/api'

export function createDefaultToolConfig(
  tools: BuiltinToolRead[],
  assistantAgents: AgentAssistantToolSelection[] = [],
): AgentToolConfig {
  const defaults = new Set(['read', 'glob', 'grep'])
  return {
    tools: Object.fromEntries(
      tools.map((tool) => [
        tool.id,
        {
          enabled: defaults.has(tool.id),
          policy: tool.policy,
        },
      ]),
    ),
    assistant_agents: assistantAgents,
  }
}

export function mergeToolConfig(
  tools: BuiltinToolRead[],
  current: AgentToolConfig | null | undefined,
  assistantAgents?: AgentAssistantToolSelection[],
): AgentToolConfig {
  const assistantSelections = assistantAgents ?? current?.assistant_agents ?? []
  const defaults = createDefaultToolConfig(tools, assistantSelections)
  if (!current) return defaults
  return {
    tools: Object.fromEntries(
      tools.map((tool) => {
        const selection = current.tools[tool.id]
        return [
          tool.id,
          {
            enabled: selection?.enabled ?? defaults.tools[tool.id]?.enabled ?? false,
            policy: selection?.policy ?? tool.policy,
          },
        ]
      }),
    ),
    assistant_agents: assistantSelections,
  }
}
