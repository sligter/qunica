import { useMutation, useQueryClient } from '@tanstack/react-query'

import { isConversationWorkspaceRootsQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { AgentRead, AgentUpdate } from '@/types/api'

export function useUpdateAgent(agentId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: AgentUpdate) =>
      fetchJson<AgentRead>(`/agents/${agentId}`, {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['agents'] })
      // Binding or unbinding the agent's workspace adds or removes a root in
      // every conversation it is a member of, not just the one on screen.
      void qc.invalidateQueries({
        predicate: (query) => isConversationWorkspaceRootsQueryKey(query.queryKey),
      })
      qc.setQueryData(['agents', agentId], updated)
    },
  })
}
