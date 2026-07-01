import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'

export function useDeleteAgent() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (agentId: string) =>
      fetchJson<void>(`/agents/${agentId}`, { token, method: 'DELETE' }),
    onSuccess: (_data, agentId) => {
      void qc.invalidateQueries({ queryKey: ['agents'] })
      void qc.invalidateQueries({ queryKey: ['group-agents'] })
      qc.removeQueries({ queryKey: ['agents', agentId] })
    },
  })
}
