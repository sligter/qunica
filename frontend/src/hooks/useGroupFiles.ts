import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { ApiError, fetchFormData, fetchJson } from '@/lib/api-v2/client'
import { isDesktopRuntime, saveFileViaDialog } from '@/lib/desktop'
import { apiUrl } from '@/lib/runtime'
import { useAuthStore } from '@/stores/authStore'
import type {
  GroupFileRead,
  GroupWorkspaceFilePreview,
  GroupWorkspaceFileRead,
  GroupWorkspaceRoot,
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

export function useGroupWorkspaceRoot(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'workspace-files', 'root'],
    queryFn: () =>
      fetchJson<GroupWorkspaceRoot>(`/groups/${groupId}/workspace-files/root`, { token }),
    enabled: token !== null && !!groupId,
    staleTime: 5 * 60_000,
    retry: false,
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

export function useUploadGroupWorkspaceFile(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (file: File) => {
      if (!groupId) throw new Error('Group is required to upload workspace files')
      const fd = new FormData()
      fd.append('file', file)
      return fetchFormData<GroupWorkspaceFileRead>(
        `/groups/${groupId}/workspace-files/upload`,
        fd,
        { token },
      )
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
    },
  })
}

export async function downloadGroupWorkspaceFile(
  groupId: string,
  path: string,
  token: string | null,
) {
  const headers: Record<string, string> = {}
  if (token) headers.Authorization = `Bearer ${token}`
  const response = await fetch(
    apiUrl(`/api/v2/groups/${groupId}/workspace-files/download?${withPath(path)}`),
    { headers },
  )
  if (!response.ok) {
    let message = `HTTP ${response.status}`
    try {
      const body = (await response.json()) as { error?: { message?: string }; detail?: string }
      message = body.error?.message ?? body.detail ?? message
    } catch {
      // Keep fallback message for non-JSON errors.
    }
    throw new ApiError(response.status, 'http_error', message)
  }
  const fileName = path.split('/').pop() || 'download'
  const blob = await response.blob()
  // Desktop: the WebView2/WKWebView download path is unreliable for blob URLs,
  // so route through a native "Save As" dialog instead.
  if (isDesktopRuntime()) {
    const bytes = new Uint8Array(await blob.arrayBuffer())
    await saveFileViaDialog(fileName, bytes)
    return
  }
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = fileName
  document.body.appendChild(link)
  link.click()
  link.remove()
  URL.revokeObjectURL(url)
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
