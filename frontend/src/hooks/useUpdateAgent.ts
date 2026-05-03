import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
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
      qc.setQueryData(['agents', agentId], updated)
    },
  })
}
