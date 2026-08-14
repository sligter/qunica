import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentRead } from '@/types/api'

export function useGroupAgents(
  groupId: string | undefined,
  threadId?: string | null,
) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'agents', threadId ?? 'all'],
    queryFn: () => fetchJson<GroupAgentRead[]>(
      `/groups/${groupId}/agents${threadId ? `?thread_id=${encodeURIComponent(threadId)}` : ''}`,
      { token },
    ),
    enabled: token !== null && groupId !== undefined && threadId !== null,
  })
}
