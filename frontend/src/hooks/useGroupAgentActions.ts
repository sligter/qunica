import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentRead } from '@/types/api'

interface AgentMutationVars {
  groupId: string
  agentId: string
}

interface AgentMuteVars extends AgentMutationVars {
  muted: boolean
}

export function useRemoveGroupAgent() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId }: AgentMutationVars) =>
      fetchJson<void>(`/groups/${groupId}/agents/${agentId}`, {
        token,
        method: 'DELETE',
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}

export function useMuteGroupAgent() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId, muted }: AgentMuteVars) =>
      fetchJson<GroupAgentRead>(`/groups/${groupId}/agents/${agentId}/mute`, {
        token,
        method: 'PATCH',
        body: { muted },
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
