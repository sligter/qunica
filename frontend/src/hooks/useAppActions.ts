import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { queryKeysForKind } from '@/lib/appActions'
import { useAuthStore } from '@/stores/authStore'
import type { AppActionRead } from '@/types/api'

export const appActionsQueryKey = ['app-actions'] as const

export function useAppActions() {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: appActionsQueryKey,
    queryFn: () => fetchJson<AppActionRead[]>('/app-actions', { token }),
    enabled: token !== null,
  })
}

/**
 * Approve or reject one staged change.
 *
 * On success the lists for the affected kind are invalidated, so the surface
 * the user is looking at reflects the change they just approved instead of
 * showing stale rows until something else happens to refetch.
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
    onSuccess: (_result, variables) => {
      void qc.invalidateQueries({ queryKey: appActionsQueryKey })
      if (variables.decision !== 'approve' || !variables.targetKind) return
      for (const key of queryKeysForKind(variables.targetKind)) {
        void qc.invalidateQueries({ queryKey: key })
      }
    },
  })
}
