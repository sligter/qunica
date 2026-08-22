import { Server } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useDeleteMcpServer, useMcpServers } from '@/hooks/useMcpServers'
import { useRenameResource } from '@/hooks/useRenameResource'
import type { McpTransport } from '@/types/api'

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

export function McpServersListColumn() {
  const { t } = useTranslation('mcp')
  const servers = useMcpServers()
  const rename = useRenameResource('/mcp-servers', ['mcp-servers'])
  const del = useDeleteMcpServer()

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
      icon={Server}
      items={(servers.data ?? []).map((server) => ({
        id: server.id,
        to: `/mcp-servers/${server.id}`,
        name: server.name,
        summary: `${server.transport} · ${
          server.transport === 'stdio' ? (server.command ?? '-') : (server.url ?? '-')
        }`,
        avatarClass: transportColor(server.transport),
        avatarInitial: transportInitial(server.transport),
        deleteTitle: t('detail.deleteTitle', { name: server.name }),
        deleteDescription: t('detail.deleteDescription'),
      }))}
      onRename={(item, name) => rename.mutateAsync({ id: item.id, name })}
      onDelete={(item) => del.mutateAsync(item.id)}
    />
  )
}
