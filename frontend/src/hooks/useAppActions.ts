import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { queryKeysForKind } from '@/lib/appActions'
import { useAuthStore } from '@/stores/authStore'
import type { AppActionList, AppActionRead } from '@/types/api'

export const appActionsQueryKey = ['app-actions'] as const

export function useAppActions(options: { limit?: number; skip?: number } = {}) {
  const token = useAuthStore((state) => state.token)
  const { limit = 50, skip = 0 } = options
  return useQuery({
    queryKey: [...appActionsQueryKey, limit, skip],
    queryFn: () => {
      const search = new URLSearchParams({ limit: String(limit), skip: String(skip) })
      return fetchJson<AppActionList>(`/app-actions?${search.toString()}`, { token })
    },
    enabled: token !== null,
    refetchInterval: (query) =>
      query.state.data?.items?.some((action) => action.status === 'approved')
        ? 1_000
        : false,
  })
}

/**
 * Approve or reject one staged change.
 *
 * Always refresh after a decision attempt. The server may have committed the
 * action even if the response was lost, and keeping stale message history in
 * that case makes a successful chat action look like it never replied.
 */
export function useResolveAppAction() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      actionId,
      decision,
    }: {
      actionId: string
      decision: 'approve' | 'reject'
      targetKind?: string
    }) =>
      fetchJson<AppActionRead>(`/app-actions/${actionId}/${decision}`, {
        method: 'POST',
        token,
      }),
    onSettled: (_result, _error, variables) => {
      void qc.invalidateQueries({ queryKey: appActionsQueryKey })
      if (variables.decision !== 'approve' || !variables.targetKind) return
      for (const key of queryKeysForKind(variables.targetKind)) {
        void qc.invalidateQueries({ queryKey: key })
      }
    },
  })
}

export function useDeleteAppAction() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (actionId: string) =>
      fetchJson<void>(`/app-actions/${actionId}`, { method: 'DELETE', token }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: appActionsQueryKey })
    },
  })
}

export function useClearAppActions() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => fetchJson<void>('/app-actions', { method: 'DELETE', token }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: appActionsQueryKey })
    },
  })
}
