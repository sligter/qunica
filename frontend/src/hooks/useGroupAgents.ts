import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/http'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentRead } from '@/types/api'

export function useGroupAgents(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'agents'],
    queryFn: () => fetchJson<GroupAgentRead[]>(`/groups/${groupId}/agents`, { token }),
    enabled: token !== null && groupId !== undefined,
  })
}
