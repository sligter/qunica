import { Folder } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ListColumn } from '@/components/layout/ListColumn'
import { useDeleteWorkspace, useWorkspaces } from '@/hooks/useWorkspaces'
import { useRenameResource } from '@/hooks/useRenameResource'
import { avatarColorClass } from '@/lib/avatarColor'
import type { WorkspaceRead } from '@/types/api'

function workspaceSummary(
  workspace: WorkspaceRead,
  missing: { local: string; sandbox: string },
): string {
  if (workspace.backend_type === 'local') {
    return workspace.local_path ?? missing.local
  }
  return workspace.sandbox_ref ?? missing.sandbox
}

export function WorkspacesListColumn() {
  const { t } = useTranslation('workspaces')
  const workspaces = useWorkspaces()
  const rename = useRenameResource('/workspaces', ['workspaces'])
  const del = useDeleteWorkspace()

  return (
    <ListColumn
      title={t('title')}
      newTo="/workspaces/new"
      newLabel={t('new')}
      searchPlaceholder={t('search')}
      isLoading={workspaces.isLoading}
      loadError={!!workspaces.error}
      errorText={t('loadError')}
      emptyText={t('empty')}
      icon={Folder}
      items={(workspaces.data ?? []).map((w) => ({
        id: w.id,
        to: `/workspaces/${w.id}`,
        name: w.name,
        summary: workspaceSummary(w, {
          local: t('noLocalPath'),
          sandbox: t('noSandboxReference'),
        }),
        avatarClass: avatarColorClass(w.id),
        avatarInitial: w.name.slice(0, 1).toUpperCase(),
        deleteTitle: t('detail.deleteTitle', { name: w.name }),
        deleteDescription: t('detail.deleteDescription'),
      }))}
      onRename={(item, name) => rename.mutateAsync({ id: item.id, name })}
      onDelete={(item) => del.mutateAsync(item.id)}
    />
  )
}
