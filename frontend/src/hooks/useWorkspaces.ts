import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/http'
import { useAuthStore } from '@/stores/authStore'
import type { WorkspaceCreate, WorkspaceRead, WorkspaceUpdate } from '@/types/api'

type WorkspaceReadV2 = Omit<WorkspaceRead, 'local_path' | 'sandbox_ref'> & {
  local_path: string | null
  sandbox_ref?: string | null
}

type WorkspaceCreateV2 = Omit<WorkspaceCreate, 'sandbox_ref'>
type WorkspaceUpdateV2 = Omit<WorkspaceUpdate, 'sandbox_ref'>

function normalizeWorkspace(workspace: WorkspaceReadV2): WorkspaceRead {
  if (workspace.backend_type === 'cloud_sandbox') {
    return {
      ...workspace,
      local_path: null,
      sandbox_ref: workspace.sandbox_ref ?? workspace.local_path ?? null,
    }
  }

  return {
    ...workspace,
    sandbox_ref: null,
  }
}

function workspaceCreateBody(data: WorkspaceCreate): WorkspaceCreateV2 {
  const { sandbox_ref, ...body } = data
  if (data.backend_type !== 'local' && sandbox_ref !== undefined) {
    return {
      ...body,
      local_path: sandbox_ref,
    }
  }
  return body
}

function workspaceUpdateBody(data: WorkspaceUpdate): WorkspaceUpdateV2 {
  const { sandbox_ref, ...body } = data
  if (data.backend_type !== 'local' && sandbox_ref !== undefined) {
    return {
      ...body,
      local_path: sandbox_ref,
    }
  }
  return body
}

export function useWorkspaces() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['workspaces'],
    queryFn: async () => {
      const workspaces = await fetchJson<WorkspaceReadV2[]>('/workspaces', { token })
      return workspaces.map(normalizeWorkspace)
    },
    enabled: token !== null,
  })
}

export function useCreateWorkspace() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: async (data: WorkspaceCreate) => {
      const workspace = await fetchJson<WorkspaceReadV2>('/workspaces', {
        token,
        method: 'POST',
        body: workspaceCreateBody(data),
      })
      return normalizeWorkspace(workspace)
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['workspaces'] })
    },
  })
}

export function useUpdateWorkspace(workspaceId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: async (data: WorkspaceUpdate) => {
      const workspace = await fetchJson<WorkspaceReadV2>(`/workspaces/${workspaceId}`, {
        token,
        method: 'PATCH',
        body: workspaceUpdateBody(data),
      })
      return normalizeWorkspace(workspace)
    },
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['workspaces'] })
      qc.setQueryData(['workspaces', workspaceId], updated)
    },
  })
}

export function useDeleteWorkspace() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (workspaceId: string) =>
      fetchJson<void>(`/workspaces/${workspaceId}`, {
        token,
        method: 'DELETE',
      }),
    onSuccess: (_data, workspaceId) => {
      void qc.invalidateQueries({ queryKey: ['workspaces'] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
      void qc.invalidateQueries({ queryKey: ['agents'] })
      qc.removeQueries({ queryKey: ['workspaces', workspaceId] })
    },
  })
}
