import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupRead } from '@/types/api'

export function useGroups() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups'],
    queryFn: () => fetchJson<GroupRead[]>('/groups', { token }),
    enabled: token !== null,
  })
}

export function useGroup(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId],
    queryFn: () => fetchJson<GroupRead>(`/groups/${groupId}`, { token }),
    enabled: token !== null && groupId !== undefined,
  })
}
