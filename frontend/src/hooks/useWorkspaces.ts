import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { WorkspaceCreate, WorkspaceRead, WorkspaceUpdate } from '@/types/api'

export function useWorkspaces() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['workspaces'],
    queryFn: () => fetchJson<WorkspaceRead[]>('/workspaces', { token }),
    enabled: token !== null,
  })
}

export function useCreateWorkspace() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: WorkspaceCreate) =>
      fetchJson<WorkspaceRead>('/workspaces', {
        token,
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['workspaces'] })
    },
  })
}

export function useUpdateWorkspace(workspaceId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: WorkspaceUpdate) =>
      fetchJson<WorkspaceRead>(`/workspaces/${workspaceId}`, {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['workspaces'] })
      qc.setQueryData(['workspaces', workspaceId], updated)
    },
  })
}
