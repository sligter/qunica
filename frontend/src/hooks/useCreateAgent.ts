import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { AgentCreate, AgentRead } from '@/types/api'

export function useCreateAgent() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: (input: AgentCreate) =>
      fetchJson<AgentRead>('/agents', { method: 'POST', body: input, token }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['agents'] })
    },
  })
}
