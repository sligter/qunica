import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useMcpServers } from '@/hooks/useMcpServers'
import type { McpTransport } from '@/types/api'

interface McpServersListColumnProps {
  width?: number
}

/** One colour per transport, so the list reads at a glance. */
function transportColor(transport: McpTransport): string {
  if (transport === 'stdio') return 'bg-avatar-1 text-avatar-foreground'
  if (transport === 'sse') return 'bg-avatar-2 text-avatar-foreground'
  return 'bg-avatar-3 text-avatar-foreground'
}

function transportInitial(transport: McpTransport): string {
  if (transport === 'stdio') return 'S'
  if (transport === 'sse') return 'E'
  return 'H'
}

export function McpServersListColumn({ width }: McpServersListColumnProps) {
  const { t } = useTranslation('mcp')
  const servers = useMcpServers()

  return (
    <ListColumn
      title={t('title')}
      newTo="/mcp-servers/new"
      newLabel={t('new')}
      searchPlaceholder={t('search')}
      isLoading={servers.isLoading}
      loadError={!!servers.error}
      errorText={t('loadError')}
      emptyText={t('empty')}
      width={width}
      items={(servers.data ?? []).map((server) => ({
        id: server.id,
        to: `/mcp-servers/${server.id}`,
        name: server.name,
        summary: `${server.transport} · ${
          server.transport === 'stdio' ? (server.command ?? '-') : (server.url ?? '-')
        }`,
        avatarClass: transportColor(server.transport),
        avatarInitial: transportInitial(server.transport),
      }))}
    />
  )
}
