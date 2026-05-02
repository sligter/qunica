import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentRead } from '@/types/api'

interface AddAgentVars {
  groupId: string
  agentId: string
}

export function useAddAgentToGroup() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId }: AddAgentVars) =>
      fetchJson<GroupAgentRead>(`/groups/${groupId}/agents`, {
        method: 'POST',
        body: { agent_id: agentId },
        token,
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
    },
  })
}
