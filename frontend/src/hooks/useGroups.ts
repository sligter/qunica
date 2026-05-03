import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupRead, GroupUpdate } from '@/types/api'

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

export function useUpdateGroup(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: GroupUpdate) =>
      fetchJson<GroupRead>(`/groups/${groupId}`, {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
