import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentAdd, GroupAgentRead } from '@/types/api'

interface AddAgentVars {
  groupId: string
  agentId: string
  shareGroupWorkspace?: boolean
}

export function useAddAgentToGroup() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId, shareGroupWorkspace }: AddAgentVars) =>
      fetchJson<GroupAgentRead>(`/groups/${groupId}/agents`, {
        method: 'POST',
        body: {
          agent_id: agentId,
          share_group_workspace: shareGroupWorkspace ?? false,
        } satisfies GroupAgentAdd,
        token,
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
