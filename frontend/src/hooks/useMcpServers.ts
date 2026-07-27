import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  McpServerCreate,
  McpServerRead,
  McpServerUpdate,
  McpTestConnectionResult,
} from '@/types/api'

export function useMcpServers() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['mcp-servers'],
    queryFn: () => fetchJson<McpServerRead[]>('/mcp-servers', { token }),
    enabled: token !== null,
  })
}

export function useMcpServer(id: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['mcp-servers', id],
    queryFn: () => fetchJson<McpServerRead>(`/mcp-servers/${id}`, { token }),
    enabled: token !== null && !!id,
  })
}

export function useCreateMcpServer() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: McpServerCreate) =>
      fetchJson<McpServerRead>('/mcp-servers', { token, method: 'POST', body: data }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['mcp-servers'] })
    },
  })
}

export function useUpdateMcpServer(serverId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: McpServerUpdate) =>
      fetchJson<McpServerRead>(`/mcp-servers/${serverId}`, {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['mcp-servers'] })
      qc.setQueryData(['mcp-servers', serverId], updated)
      // Editing a server changes which tools it offers, so any cached probe is
      // no longer describing the server the agent would now reach.
      void qc.invalidateQueries({ queryKey: ['mcp-servers', serverId, 'tools'] })
    },
  })
}

export function useDeleteMcpServer() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      fetchJson<void>(`/mcp-servers/${id}`, { token, method: 'DELETE' }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['mcp-servers'] })
    },
  })
}

/** Connect to a saved server and list its tools. */
export function useTestMcpServer() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      fetchJson<McpTestConnectionResult>(`/mcp-servers/${id}/test`, {
        token,
        method: 'POST',
        body: {},
      }),
    onSuccess: (result, id) => {
      qc.setQueryData(['mcp-servers', id, 'tools'], result)
    },
  })
}

/** Connect to a not-yet-saved configuration and list its tools. */
export function useTestMcpDraft() {
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: (data: McpServerCreate) =>
      fetchJson<McpTestConnectionResult>('/mcp-servers/test', {
        token,
        method: 'POST',
        body: data,
      }),
  })
}

/**
 * The tools a saved server exposes, discovered on demand.
 *
 * Probing opens a real connection (and for stdio, spawns a process), so this
 * never runs on mount — the caller enables it when a panel that needs the tool
 * list is actually opened.
 */
export function useMcpServerTools(id: string | undefined, enabled: boolean) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['mcp-servers', id, 'tools'],
    queryFn: () =>
      fetchJson<McpTestConnectionResult>(`/mcp-servers/${id}/test`, {
        token,
        method: 'POST',
        body: {},
      }),
    enabled: token !== null && !!id && enabled,
    staleTime: 5 * 60 * 1000,
    retry: false,
  })
}
