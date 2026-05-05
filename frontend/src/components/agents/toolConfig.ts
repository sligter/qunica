import type { AgentToolConfig, BuiltinToolRead } from '@/types/api'

export function createDefaultToolConfig(tools: BuiltinToolRead[]): AgentToolConfig {
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
  }
}

export function mergeToolConfig(
  tools: BuiltinToolRead[],
  current: AgentToolConfig | null | undefined,
): AgentToolConfig {
  const defaults = createDefaultToolConfig(tools)
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
  }
}
