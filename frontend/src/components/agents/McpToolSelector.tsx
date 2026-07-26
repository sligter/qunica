import { useState } from 'react'
import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { EntityPicker } from '@/components/ui/entity-picker'
import { PageState } from '@/components/ui/page-state'
import { useMcpServerTools } from '@/hooks/useMcpServers'
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

  const setSelectedServers = (serverIds: string[]) => {
    const next = serverIds.map(
      (server_id) =>
        value.find((entry) => entry.server_id === server_id) ?? {
          server_id,
          enabled: true,
          tools: [],
        },
    )
    onChange(next)
    // A server that is no longer selected has nothing to narrow.
    if (expanded && !serverIds.includes(expanded)) setExpanded(null)
  }

  const setTools = (serverId: string, tools: string[]) => {
    onChange(
      value.map((entry) =>
        entry.server_id === serverId ? { ...entry, tools } : entry,
      ),
    )
  }

  const deselectServer = (serverId: string) => {
    onChange(value.filter((entry) => entry.server_id !== serverId))
    setExpanded(null)
  }

  const selectedIds = value
    .filter((entry) => entry.enabled)
    .map((entry) => entry.server_id)

  return (
    <div className="space-y-2">
      <EntityPicker
        label={t('tools.mcp.title')}
        searchPlaceholder={t('form.searchMcpServers')}
        items={servers.map((server) => ({
          id: server.id,
          label: server.name,
          // The slug is the scannable identity: it is literally what lands in
          // the model's tool namespace.
          meta: `mcp__${server.slug}__*`,
          keywords: server.description ?? undefined,
          disabledReason: server.enabled
            ? undefined
            : t('tools.mcp.serverDisabled'),
          trailing: selectionFor(server.id) ? (
            <button
              type="button"
              onClick={() =>
                setExpanded(expanded === server.id ? null : server.id)
              }
              aria-expanded={expanded === server.id}
              className="flex items-center gap-1 text-2xs text-muted-foreground hover:text-foreground"
            >
              {expanded === server.id ? (
                <ChevronDown className="h-3 w-3" />
              ) : (
                <ChevronRight className="h-3 w-3" />
              )}
              {(selectionFor(server.id)?.tools ?? []).length > 0
                ? t('tools.mcp.selectedCount', {
                    count: selectionFor(server.id)!.tools!.length,
                  })
                : t('tools.mcp.allTools')}
            </button>
          ) : undefined,
        }))}
        selectedIds={selectedIds}
        onChange={setSelectedServers}
        countLabel={(total, selected) =>
          t('form.mcpServerCount', { total, selected, count: total })
        }
        empty={
          <PageState
            inset
            icon={null}
            title={t('tools.mcp.none')}
            action={
              <Link
                to="/mcp-servers/new"
                className="text-xs font-medium text-primary hover:underline"
              >
                {t('tools.mcp.configure')}
              </Link>
            }
          />
        }
      />
      {expanded && selectionFor(expanded) ? (
        <div className="rounded-md border border-border bg-background">
          <McpToolList
            serverId={expanded}
            selected={selectionFor(expanded)?.tools ?? []}
            onChange={(tools) => setTools(expanded, tools)}
            onDeselectServer={() => deselectServer(expanded)}
          />
        </div>
      ) : null}
    </div>
  )
}

interface McpToolListProps {
  serverId: string
  selected: string[]
  onChange: (tools: string[]) => void
  /** Called when the operator narrows the server down to no tools at all. */
  onDeselectServer: () => void
}

/** The expanded per-tool checklist, probing the server for its tool list. */
function McpToolList({
  serverId,
  selected,
  onChange,
  onDeselectServer,
}: McpToolListProps) {
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

  const allNames = tools.map((tool) => tool.name)

  // The picker has no notion of the "all" sentinel, so expand it on the way in
  // and collapse it on the way out. Without expanding, every box would render
  // unchecked in the default state and the first click would read as an add
  // rather than a removal.
  const expanded = selected.length === 0 ? allNames : selected

  const setSelection = (next: string[]) => {
    if (next.length === 0) {
      // An empty list already means "all", so it cannot also mean "none".
      // Clearing every tool means the agent should not use this server, which
      // is what deselecting it says unambiguously.
      onDeselectServer()
      return
    }
    // Collapse back to the sentinel once everything is checked, so a tool the
    // server gains later is picked up instead of being silently excluded by a
    // frozen list.
    onChange(next.length === allNames.length ? [] : next)
  }

  return (
    <div className="space-y-2 px-3 py-3">
      <p className="text-2xs text-muted-foreground">{t('tools.mcp.narrowHint')}</p>
      <EntityPicker
        label={t('tools.mcp.toolList')}
        searchPlaceholder={t('tools.mcp.searchTools')}
        items={tools.map((tool) => ({
          id: tool.name,
          label: tool.name,
          monoLabel: true,
          meta: tool.description || undefined,
          keywords: tool.exposed_name,
        }))}
        selectedIds={expanded}
        onChange={setSelection}
        countLabel={(total, selectedCount) =>
          t('tools.mcp.toolCount', {
            total,
            selected: selectedCount,
            count: total,
          })
        }
      />
    </div>
  )
}
