import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { parseGroupTurnTrace } from '@/lib/api-v2/schemas'
import type { GroupTurnTraceResponse } from '@/lib/api-v2/types'
import { useAuthStore } from '@/stores/authStore'

export function groupTurnTraceQueryKey(
  groupId: string | undefined,
  turnId: string | null,
) {
  return ['groups', groupId, 'turns', turnId] as const
}

async function fetchGroupTurnTrace(
  groupId: string,
  turnId: string,
  token: string,
): Promise<GroupTurnTraceResponse> {
  const response = await fetchJson<unknown>(`/groups/${groupId}/turns/${turnId}`, {
    token,
  })
  return parseGroupTurnTrace(response)
}

export function useGroupTurnTrace(groupId: string | undefined, turnId: string | null) {
  const token = useAuthStore((state) => state.token)

  return useQuery({
    queryKey: groupTurnTraceQueryKey(groupId, turnId),
    queryFn: () => fetchGroupTurnTrace(groupId!, turnId!, token!),
    enabled: token !== null && groupId !== undefined && turnId !== null,
  })
}

export function useCancelGroupTurn(groupId: string | undefined, turnId: string | null) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async () => {
      if (!token || !groupId || !turnId) {
        throw new Error('A signed-in group turn is required to cancel')
      }
      const response = await fetchJson<unknown>(
        `/groups/${groupId}/turns/${turnId}/cancel`,
        { method: 'POST', token },
      )
      return parseGroupTurnTrace(response)
    },
    onSuccess: (trace) => {
      queryClient.setQueryData(groupTurnTraceQueryKey(groupId, turnId), trace)
    },
  })
}
