import { useEffect, useMemo } from 'react'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import type { InfiniteData } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ClearGroupMessagesResponse, Message, MessageSendResponse } from '@/types/api'

const MESSAGE_PAGE_SIZE = 30
const INITIAL_PAGE_PARAM: string | undefined = undefined

function emptyMessagePages(): InfiniteData<Message[], string | undefined> {
  return {
    // Infinite queries must keep the pages/pageParams envelope even when empty.
    pages: [[]],
    pageParams: [INITIAL_PAGE_PARAM],
  }
}

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
        qc.setQueryData(['groups', groupId, 'messages'], emptyMessagePages())
        clearGroupMessages(groupId)
        void qc.invalidateQueries({ queryKey: ['groups', groupId, 'messages'] })
        // The backend resets each agent's last-known context usage on clear;
        // refetch so the avatar ring/tooltip drop the stale baseline immediately.
        void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
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

/**
 * Fetches the historical message list and primes the messageStore. Realtime
 * updates flow through the SSE hook, not through this query.
 */
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
    initialPageParam: INITIAL_PAGE_PARAM,
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
