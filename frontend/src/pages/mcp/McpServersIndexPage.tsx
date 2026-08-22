import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Globe, Plus, Server, Terminal } from 'lucide-react'
import { Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { SearchInput } from '@/components/ui/search-input'
import { useMcpServers } from '@/hooks/useMcpServers'
import type { McpTransport } from '@/types/api'

function transportBadgeClass(transport: McpTransport): string {
  if (transport === 'stdio') {
    return 'bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20'
  }
  if (transport === 'sse') {
    return 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20'
  }
  return 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/20'
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
      <div className="space-y-6">
        {/* Metric Cards Row */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('mcp:title')}</span>
              <Server className="h-4 w-4 text-primary/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{list.length}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('mcp:fields.enabled')}</span>
              <span className="h-2 w-2 rounded-full bg-success ring-4 ring-success/20" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-success">{enabledCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">stdio</span>
              <Terminal className="h-4 w-4 text-info/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-info">{stdioCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">HTTP / SSE</span>
              <Globe className="h-4 w-4 text-amber-500/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-amber-500">{httpCount}</p>
          </div>
        </div>

        {/* Gallery Grid or Empty State */}
        {list.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 bg-card/30 p-12 text-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary">
              <Server className="h-6 w-6" />
            </div>
            <h3 className="text-base font-semibold">{t('mcp:empty')}</h3>
            <p className="mt-1 max-w-sm text-sm text-muted-foreground">
              {t('mcp:form.createSubtitle')}
            </p>
            <Button className="mt-6 gap-2" asChild>
              <Link to="/mcp-servers/new">
                <Plus className="h-4 w-4" />
                {t('mcp:new')}
              </Link>
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-2 xl:grid-cols-3">
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
              <p className="col-span-full py-12 text-center text-sm text-muted-foreground">
                {t('mcp:noMatches', '没有匹配的 MCP 服务。')}
              </p>
            ) : null}
          </div>
        )}
      </div>
    </DetailShell>
  )
}
