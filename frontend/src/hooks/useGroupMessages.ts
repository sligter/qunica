import { useEffect } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ClearGroupMessagesResponse, Message } from '@/types/api'

/**
 * Fetches the historical message list and primes the messageStore. Realtime
 * updates flow through the SSE hook, not through this query.
 */
export function useClearGroupMessages(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  const clearGroupMessages = useMessageStore((s) => s.clearGroupMessages)
  return useMutation({
    mutationFn: () =>
      fetchJson<ClearGroupMessagesResponse>(`/groups/${groupId}/messages/clear`, {
        method: 'POST',
        token,
      }),
    onSuccess: () => {
      if (groupId) {
        qc.setQueryData(['groups', groupId, 'messages'], [])
        clearGroupMessages(groupId)
        void qc.invalidateQueries({ queryKey: ['groups', groupId, 'messages'] })
      }
    },
  })
}

export function useGroupMessages(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const setHistory = useMessageStore((s) => s.setHistory)

  const query = useQuery({
    queryKey: ['groups', groupId, 'messages'],
    queryFn: () => fetchJson<Message[]>(`/groups/${groupId}/messages`, { token }),
    enabled: token !== null && groupId !== undefined,
  })

  useEffect(() => {
    if (groupId && query.data) {
      setHistory(groupId, query.data)
    }
  }, [groupId, query.data, setHistory])

  return query
}
