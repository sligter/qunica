import { ListColumn } from '@/components/layout/ListColumn'
import { useAgents } from '@/hooks/useAgents'
import { avatarColorClass } from '@/lib/avatarColor'

interface AgentsListColumnProps {
  width?: number
}

export function AgentsListColumn({ width }: AgentsListColumnProps) {
  const agents = useAgents()

  return (
    <ListColumn
      title="Agents"
      newTo="/agents/new"
      newLabel="New agent"
      searchPlaceholder="Search agents"
      isLoading={agents.isLoading}
      loadError={!!agents.error}
      errorText="Failed to load agents."
      emptyText="No agents yet. Click + to create one."
      width={width}
      items={(agents.data ?? []).map((a) => ({
        id: a.id,
        to: `/agents/${a.id}`,
        name: a.name,
        summary:
          a.runtime_kind === 'acp'
            ? 'ACP runtime'
            : a.description || a.system_prompt,
        avatarClass: avatarColorClass(a.id),
        avatarInitial: a.name.slice(0, 1).toUpperCase(),
      }))}
    />
  )
}
