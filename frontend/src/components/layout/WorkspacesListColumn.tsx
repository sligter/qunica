import { ListColumn } from '@/components/layout/ListColumn'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { avatarColorClass } from '@/lib/avatarColor'
import type { WorkspaceRead } from '@/types/api'

interface WorkspacesListColumnProps {
  width?: number
}

function workspaceSummary(workspace: WorkspaceRead): string {
  if (workspace.backend_type === 'local') {
    return workspace.local_path ?? 'No local path'
  }
  return workspace.sandbox_ref ?? 'No sandbox reference'
}

export function WorkspacesListColumn({ width }: WorkspacesListColumnProps) {
  const workspaces = useWorkspaces()

  return (
    <ListColumn
      title="Workspace"
      newTo="/settings/workspaces/new"
      newLabel="New workspace"
      searchPlaceholder="Search workspaces"
      isLoading={workspaces.isLoading}
      loadError={!!workspaces.error}
      errorText="Failed to load workspaces."
      emptyText="No workspaces yet. Click + to create one."
      width={width}
      items={(workspaces.data ?? []).map((w) => ({
        id: w.id,
        to: `/settings/workspaces/${w.id}`,
        name: w.name,
        summary: workspaceSummary(w),
        avatarClass: avatarColorClass(w.id),
        avatarInitial: w.name.slice(0, 1).toUpperCase(),
      }))}
    />
  )
}
