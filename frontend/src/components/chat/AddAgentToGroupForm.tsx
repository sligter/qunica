import { useMemo, useState } from 'react'

import { Button } from '@/components/ui/button'
import { useAddAgentToGroup } from '@/hooks/useAddAgentToGroup'
import { useAgents } from '@/hooks/useAgents'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { ApiError } from '@/lib/api-v2/client'

interface AddAgentToGroupFormProps {
  groupId: string
}

export function AddAgentToGroupForm({ groupId }: AddAgentToGroupFormProps) {
  const myAgents = useAgents()
  const inGroup = useGroupAgents(groupId)
  const addAgent = useAddAgentToGroup()
  const [error, setError] = useState<string | null>(null)

  const candidates = useMemo(() => {
    const inGroupIds = new Set((inGroup.data ?? []).map((g) => g.agent_id))
    return (myAgents.data ?? []).filter((a) => !inGroupIds.has(a.id))
  }, [inGroup.data, myAgents.data])

  if (candidates.length === 0) {
    return null
  }

  const onAdd = (agentId: string) => {
    setError(null)
    addAgent.mutate(
      { groupId, agentId },
      {
        onError: (err) => {
          setError(err instanceof ApiError ? err.message : 'Failed to add agent')
        },
      },
    )
  }

  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <span className="text-muted-foreground">Add agent:</span>
      {candidates.map((a) => (
        <Button
          key={a.id}
          size="sm"
          variant="outline"
          onClick={() => onAdd(a.id)}
          disabled={addAgent.isPending}
        >
          + {a.name}
        </Button>
      ))}
      {error && <span className="text-red-600">{error}</span>}
    </div>
  )
}
