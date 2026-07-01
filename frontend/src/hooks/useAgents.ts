import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { AgentRead } from '@/types/api'

export function useAgents() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['agents'],
    queryFn: () => fetchJson<AgentRead[]>('/agents', { token }),
    enabled: token !== null,
  })
}

export function useAgent(agentId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['agents', agentId],
    queryFn: () => fetchJson<AgentRead>(`/agents/${agentId}`, { token }),
    enabled: token !== null && !!agentId,
  })
}
