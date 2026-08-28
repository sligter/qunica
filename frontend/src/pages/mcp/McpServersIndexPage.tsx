import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Globe, Plus, Server, Terminal } from 'lucide-react'
import { Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { DetailShell } from '@/components/layout/DetailShell'
import {
  EntityEmptyState,
  EntityIndexSkeleton,
  IndexErrorState,
  MetricCard,
  MetricRow,
  NoMatchesState,
} from '@/components/layout/EntityIndexParts'
import { Button } from '@/components/ui/button'
import { SearchInput } from '@/components/ui/search-input'
import { useMcpServers } from '@/hooks/useMcpServers'
import { TINTED_BADGE } from '@/lib/tintedBadge'
import type { McpTransport } from '@/types/api'

function transportBadgeClass(transport: McpTransport): string {
  if (transport === 'stdio') return TINTED_BADGE.violet
  if (transport === 'sse') return TINTED_BADGE.blue
  return TINTED_BADGE.amber
}

export function McpServersIndexPage() {
  const { t } = useTranslation(['mcp', 'common'])
  const servers = useMcpServers()
  const [query, setQuery] = useState('')

  const list = servers.data ?? []
  const listKey = list.map((s) => s.id).join(',')
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return list
    // Endpoint text is part of what a user matches on: half-remembered host
    // names are how people look for servers.
    return list.filter((s) => {
      const endpoint = s.transport === 'stdio' ? s.command ?? '' : s.url ?? ''
      return (
        s.name.toLowerCase().includes(q) || endpoint.toLowerCase().includes(q)
      )
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- list is stable per query cache entry; keying on ids avoids re-running the filter on unrelated renders
  }, [listKey, query])
  const enabledCount = list.filter((s) => s.enabled).length
  const stdioCount = list.filter((s) => s.transport === 'stdio').length
  const httpCount = list.length - stdioCount

  return (
    <DetailShell
      title={t('mcp:list.selectTitle')}
      subtitle={t('mcp:list.selectDescription')}
      measure="wide"
      actions={
        <>
          {list.length > 0 ? (
            <SearchInput value={query} onChange={setQuery} label={t('mcp:search')} />
          ) : null}
          <Button size="sm" asChild>
            <Link to="/mcp-servers/new">
              <Plus className="mr-1.5 h-3.5 w-3.5" />
              {t('mcp:new')}
            </Link>
          </Button>
        </>
      }
    >
      {servers.isLoading ? (
        <EntityIndexSkeleton />
      ) : servers.error ? (
        <IndexErrorState
          title={t('mcp:loadError')}
          detail={servers.error instanceof Error ? servers.error.message : undefined}
          onRetry={() => void servers.refetch()}
          retryLabel={t('common:actions.retry')}
        />
      ) : list.length === 0 ? (
        <EntityEmptyState
          icon={Server}
          title={t('mcp:empty')}
          description={t('mcp:form.createSubtitle')}
          actionLabel={
            <>
              <Plus className="h-4 w-4" />
              {t('mcp:new')}
            </>
          }
          actionTo="/mcp-servers/new"
        />
      ) : (
        <div className="space-y-6">
        {/* Metric Cards Row */}
        <MetricRow>
          <MetricCard label={t('mcp:title')} value={list.length} icon={Server} tone="primary" />
          <MetricCard
            label={t('mcp:fields.enabled')}
            value={enabledCount}
            tone="success"
            marker={<span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />}
          />
          <MetricCard label="stdio" value={stdioCount} icon={Terminal} tone="info" />
          <MetricCard label="HTTP / SSE" value={httpCount} icon={Globe} tone="warning" />
        </MetricRow>

        {/* Gallery Grid */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
            {filtered.map((server) => {
              const endpoint =
                server.transport === 'stdio' ? server.command ?? '-' : server.url ?? '-'
              return (
                <EntityCard
                  key={server.id}
                  to={`/mcp-servers/${server.id}`}
                  editTo={`/mcp-servers/${server.id}?edit=1`}
                  title={server.name}
                  avatarIcon={<Server className="h-5 w-5" />}
                  avatarClass="bg-primary/10 text-primary"
                  statusLabel={server.enabled ? t('mcp:states.enabled') : t('mcp:states.disabled')}
                  statusActive={server.enabled}
                  metaBadge={{
                    label: server.transport,
                    className: transportBadgeClass(server.transport),
                  }}
                  description={<code className="font-mono text-2xs">{endpoint}</code>}
                  stats={[
                    {
                      key: 'tools',
                      icon: Wrench,
                      content:
                        server.tool_filter.length > 0
                          ? server.tool_filter.length
                          : t('mcp:states.allTools'),
                    },
                  ]}
                />
              )
            })}
            {filtered.length === 0 ? (
              <NoMatchesState message={t('mcp:noMatches', '没有匹配的 MCP 服务。')} />
            ) : null}
          </div>
        </div>
      )}
    </DetailShell>
  )
}
