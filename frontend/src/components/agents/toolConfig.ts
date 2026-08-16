import type {
  AgentAssistantToolSelection,
  AgentMcpServerSelection,
  AgentToolConfig,
  BuiltinToolRead,
} from '@/types/api'

export function createDefaultToolConfig(
  tools: BuiltinToolRead[],
  assistantAgents: AgentAssistantToolSelection[] = [],
  mcpServers: AgentMcpServerSelection[] = [],
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
    // MCP servers are opt-in per agent: an agent that was never told to use one
    // must not silently gain tools when the owner configures a server.
    mcp_servers: mcpServers,
    bypass_approvals: false,
  }
}

export function mergeToolConfig(
  tools: BuiltinToolRead[],
  current: AgentToolConfig | null | undefined,
  assistantAgents?: AgentAssistantToolSelection[],
): AgentToolConfig {
  const assistantSelections = assistantAgents ?? current?.assistant_agents ?? []
  // MCP selections are carried through untouched: the catalog this merge is
  // reconciling against covers built-in tools only, so a server the form has
  // not loaded yet must not be dropped from the config it is about to save.
  const mcpSelections = current?.mcp_servers ?? []
  const defaults = createDefaultToolConfig(tools, assistantSelections, mcpSelections)
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
    mcp_servers: mcpSelections,
    // Carried through rather than re-derived from the catalog: this is the
    // owner's standing answer about the agent, not something a tool list knows.
    bypass_approvals: current.bypass_approvals ?? false,
  }
}
