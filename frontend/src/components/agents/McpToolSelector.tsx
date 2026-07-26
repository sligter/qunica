import { useState } from 'react'
import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { useMcpServerTools } from '@/hooks/useMcpServers'
import { cn } from '@/lib/utils'
import type { AgentMcpServerSelection, McpServerRead } from '@/types/api'

interface McpToolSelectorProps {
  servers: McpServerRead[]
  value: AgentMcpServerSelection[]
  onChange: (next: AgentMcpServerSelection[]) => void
}

/**
 * Per-agent MCP server selection.
 *
 * A selected server exposes every tool it lists unless the operator narrows it,
 * which is why an empty `tools` array means "all". Narrowing needs the server's
 * real tool list, so expanding a row probes the server on demand rather than
 * probing every configured server whenever this form opens.
 */
export function McpToolSelector({ servers, value, onChange }: McpToolSelectorProps) {
  const { t } = useTranslation('agents')
  const [expanded, setExpanded] = useState<string | null>(null)

  const selectionFor = (serverId: string): AgentMcpServerSelection | undefined =>
    value.find((entry) => entry.server_id === serverId && entry.enabled)

  const toggleServer = (serverId: string) => {
    const enabled = selectionFor(serverId) !== undefined
    const without = value.filter((entry) => entry.server_id !== serverId)
    onChange(enabled ? without : [...without, { server_id: serverId, enabled: true, tools: [] }])
    if (enabled && expanded === serverId) setExpanded(null)
  }

  const setTools = (serverId: string, tools: string[]) => {
    onChange(
      value.map((entry) =>
        entry.server_id === serverId ? { ...entry, tools } : entry,
      ),
    )
  }

  if (servers.length === 0) {
    return (
      <div className="rounded-md border border-border bg-background p-3">
        <p className="text-xs text-muted-foreground">{t('tools.mcp.none')}</p>
        <Link
          to="/mcp-servers/new"
          className="mt-2 inline-block text-xs font-medium text-primary hover:underline"
        >
          {t('tools.mcp.configure')}
        </Link>
      </div>
    )
  }

  return (
    <div className="space-y-2">
      {servers.map((server) => {
        const selection = selectionFor(server.id)
        const isOpen = expanded === server.id
        return (
          <div
            key={server.id}
            className={cn(
              'rounded-md border transition-colors',
              selection ? 'border-primary bg-primary/5' : 'border-border bg-background',
            )}
          >
            <div className="flex items-start gap-3 p-3">
              <button
                type="button"
                onClick={() => toggleServer(server.id)}
                className="min-w-0 flex-1 text-left"
                aria-pressed={selection !== undefined}
              >
                <span className="block text-sm font-medium">{server.name}</span>
                <span className="mt-1 block font-mono text-2xs text-muted-foreground">
                  mcp__{server.slug}__*
                </span>
                {server.description ? (
                  <span className="mt-1 block text-xs text-muted-foreground">
                    {server.description}
                  </span>
                ) : null}
                {!server.enabled ? (
                  <span className="mt-1 block text-2xs font-medium text-warning-foreground">
                    {t('tools.mcp.serverDisabled')}
                  </span>
                ) : null}
              </button>
              {selection ? (
                <button
                  type="button"
                  onClick={() => setExpanded(isOpen ? null : server.id)}
                  className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                >
                  {isOpen ? (
                    <ChevronDown className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronRight className="h-3.5 w-3.5" />
                  )}
                  {selection.tools && selection.tools.length > 0
                    ? t('tools.mcp.selectedCount', { count: selection.tools.length })
                    : t('tools.mcp.allTools')}
                </button>
              ) : null}
            </div>
            {selection && isOpen ? (
              <McpToolList
                serverId={server.id}
                selected={selection.tools ?? []}
                onChange={(tools) => setTools(server.id, tools)}
              />
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

interface McpToolListProps {
  serverId: string
  selected: string[]
  onChange: (tools: string[]) => void
}

/** The expanded per-tool checklist, probing the server for its tool list. */
function McpToolList({ serverId, selected, onChange }: McpToolListProps) {
  const { t } = useTranslation('agents')
  const probe = useMcpServerTools(serverId, true)

  if (probe.isLoading) {
    return (
      <p className="flex items-center gap-2 border-t border-border px-3 py-3 text-xs text-muted-foreground">
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        {t('tools.mcp.loading')}
      </p>
    )
  }

  if (probe.error || !probe.data?.ok) {
    return (
      <p className="border-t border-border px-3 py-3 text-xs text-destructive">
        {probe.data?.error ?? t('tools.mcp.probeFailed')}
      </p>
    )
  }

  const tools = probe.data.tools
  if (tools.length === 0) {
    return (
      <p className="border-t border-border px-3 py-3 text-xs text-muted-foreground">
        {t('tools.mcp.noTools')}
      </p>
    )
  }

  const toggle = (name: string) => {
    onChange(
      selected.includes(name)
        ? selected.filter((entry) => entry !== name)
        : [...selected, name],
    )
  }

  return (
    <div className="space-y-2 border-t border-border px-3 py-3">
      <div className="flex items-center justify-between">
        <p className="text-2xs text-muted-foreground">{t('tools.mcp.narrowHint')}</p>
        {selected.length > 0 ? (
          <button
            type="button"
            onClick={() => onChange([])}
            className="text-2xs font-medium text-primary hover:underline"
          >
            {t('tools.mcp.selectAll')}
          </button>
        ) : null}
      </div>
      <div className="grid gap-1.5 sm:grid-cols-2">
        {tools.map((tool) => {
          // No explicit selection means every tool is exposed, so every box
          // reads as checked rather than showing an all-empty list.
          const checked = selected.length === 0 || selected.includes(tool.name)
          return (
            <label
              key={tool.name}
              className="flex cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-muted"
            >
              <input
                type="checkbox"
                className="mt-0.5"
                checked={checked}
                onChange={() => toggle(tool.name)}
              />
              <span className="min-w-0">
                <span className="block font-mono">{tool.name}</span>
                {tool.description ? (
                  <span className="block truncate text-muted-foreground">
                    {tool.description}
                  </span>
                ) : null}
              </span>
            </label>
          )
        })}
      </div>
    </div>
  )
}
