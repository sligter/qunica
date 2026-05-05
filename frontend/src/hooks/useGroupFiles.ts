import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchFormData, fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type {
  GroupFileRead,
  GroupWorkspaceFilePreview,
  GroupWorkspaceFileRead,
} from '@/types/api'

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

export function workspaceFilesQueryKey(groupId: string | undefined, path = '') {
  return ['groups', groupId, 'workspace-files', path] as const
}

function withPath(path: string) {
  return `path=${encodeURIComponent(path)}`
}

export function useGroupWorkspaceFiles(groupId: string | undefined, path = '') {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: workspaceFilesQueryKey(groupId, path),
    queryFn: () =>
      fetchJson<GroupWorkspaceFileRead[]>(
        `/groups/${groupId}/workspace-files?${withPath(path)}`,
        { token },
      ),
    enabled: token !== null && !!groupId,
    refetchInterval: 10_000,
  })
}

export function useGroupWorkspaceFilePreview(
  groupId: string | undefined,
  path: string | null,
) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'workspace-files', 'preview', path],
    queryFn: () =>
      fetchJson<GroupWorkspaceFilePreview>(
        `/groups/${groupId}/workspace-files/preview?${withPath(path ?? '')}`,
        { token },
      ),
    enabled: token !== null && !!groupId && !!path,
  })
}

export function useRenameGroupWorkspaceFile(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ path, newPath }: { path: string; newPath: string }) => {
      if (!groupId) throw new Error('Group is required to rename workspace files')
      return fetchJson<GroupWorkspaceFileRead>(
        `/groups/${groupId}/workspace-files/rename?${withPath(path)}`,
        { token, method: 'PATCH', body: { new_path: newPath } },
      )
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
    },
  })
}

export function useDeleteGroupWorkspaceFile(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (path: string) => {
      if (!groupId) throw new Error('Group is required to delete workspace files')
      return fetchJson<void>(`/groups/${groupId}/workspace-files?${withPath(path)}`, {
        token,
        method: 'DELETE',
      })
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
    },
  })
}
