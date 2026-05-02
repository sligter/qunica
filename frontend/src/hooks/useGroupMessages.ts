import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

/**
 * Fetches the historical message list and primes the messageStore. Realtime
 * updates flow through the SSE hook, not through this query.
 */
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
