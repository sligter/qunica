import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchFormData, fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupFileRead } from '@/types/api'

export function useGroupFiles(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'files'],
    queryFn: () => fetchJson<GroupFileRead[]>(`/groups/${groupId}/files`, { token }),
    enabled: token !== null && !!groupId,
  })
}

export function useUploadGroupFile(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (file: File) => {
      const fd = new FormData()
      fd.append('file', file)
      return fetchFormData<GroupFileRead>(`/groups/${groupId}/files`, fd, { token })
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'files'] })
    },
  })
}

export function useDeleteGroupFile(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (fileId: string) =>
      fetchJson<void>(`/groups/${groupId}/files/${fileId}`, { token, method: 'DELETE' }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'files'] })
    },
  })
}
