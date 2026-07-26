import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentAdd, GroupAgentRead, GroupWorkspaceMode } from '@/types/api'

interface AddAgentVars {
  groupId: string
  agentId: string
  workspaceMode?: GroupWorkspaceMode
}

export function useAddAgentToGroup() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId, workspaceMode }: AddAgentVars) => {
      const body = {
        agent_id: agentId,
        ...(workspaceMode === undefined ? {} : { workspace_mode: workspaceMode }),
      } satisfies GroupAgentAdd
      return fetchJson<GroupAgentRead>(`/groups/${groupId}/agents`, {
        method: 'POST',
        body,
        token,
      })
    },
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
