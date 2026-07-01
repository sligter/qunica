import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'

export function useDeleteGroup() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (groupId: string) =>
      fetchJson<void>(`/groups/${groupId}`, { token, method: 'DELETE' }),
    onSuccess: (_, groupId) => {
      void qc.invalidateQueries({ queryKey: ['groups'] })
      qc.removeQueries({ queryKey: ['groups', groupId] })
    },
  })
}
