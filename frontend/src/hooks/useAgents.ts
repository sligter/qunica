import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
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
