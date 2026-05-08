import { useEffect, useMemo } from 'react'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ClearGroupMessagesResponse, Message, MessageSendResponse } from '@/types/api'

const MESSAGE_PAGE_SIZE = 30

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

export function useSendGroupMessage() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ groupId, content }: { groupId: string; content: string }) =>
      fetchJson<MessageSendResponse>(`/groups/${groupId}/messages`, {
        method: 'POST',
        token,
        body: { content },
      }),
    onSuccess: (_, variables) => {
      void qc.invalidateQueries({ queryKey: ['groups', variables.groupId, 'messages'] })
    },
  })
}

export function useGroupMessages(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const setHistory = useMessageStore((s) => s.setHistory)

  const query = useInfiniteQuery({
    queryKey: ['groups', groupId, 'messages'],
    queryFn: ({ pageParam }: { pageParam?: string }) => {
      const params = new URLSearchParams({ limit: String(MESSAGE_PAGE_SIZE) })
      if (pageParam) params.set('before', pageParam)
      return fetchJson<Message[]>(`/groups/${groupId}/messages?${params.toString()}`, { token })
    },
    enabled: token !== null && groupId !== undefined,
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) =>
      lastPage.length === MESSAGE_PAGE_SIZE ? lastPage[0]?.id : undefined,
  })

  const messages = useMemo(
    () => [...(query.data?.pages ?? [])].reverse().flat(),
    [query.data?.pages],
  )

  useEffect(() => {
    if (groupId && query.data) {
      setHistory(groupId, messages)
    }
  }, [groupId, messages, query.data, setHistory])

  return query
}
