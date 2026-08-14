import { useEffect, useMemo } from 'react'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import type { InfiniteData } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type { ClearGroupMessagesResponse, Message, MessageSendResponse } from '@/types/api'

const MESSAGE_PAGE_SIZE = 30
const INITIAL_PAGE_PARAM: string | undefined = undefined

export type ConversationScope = 'groups' | 'direct-chats'

export const conversationMessagesKey = (
  scope: ConversationScope,
  id: string | undefined,
  threadId?: string,
) => threadId
  ? [scope, id, 'messages', threadId] as const
  : [scope, id, 'messages'] as const

export const conversationApiPath = (scope: ConversationScope, id: string | undefined) =>
  `/${scope}/${id}`

function emptyMessagePages(): InfiniteData<Message[], string | undefined> {
  return {
    // Infinite queries must keep the pages/pageParams envelope even when empty.
    pages: [[]],
    pageParams: [INITIAL_PAGE_PARAM],
  }
}

function removeMessageFromPages(
  data: InfiniteData<Message[], string | undefined> | undefined,
  messageId: string,
): InfiniteData<Message[], string | undefined> | undefined {
  if (!data) return data
  return {
    ...data,
    pages: data.pages.map((page) => page.filter((message) => message.id !== messageId)),
  }
}

export function useClearConversationMessages(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  const clearGroupMessages = useMessageStore((s) => s.clearGroupMessages)
  return useMutation({
    mutationFn: () =>
      fetchJson<ClearGroupMessagesResponse>(
        `${conversationApiPath(scope, conversationId)}/messages/clear`,
        {
          method: 'POST',
          token,
        },
      ),
    onSuccess: () => {
      if (conversationId) {
        const messagesKey = conversationMessagesKey(scope, conversationId)
        qc.setQueryData(messagesKey, emptyMessagePages())
        clearGroupMessages(conversationId)
        void qc.invalidateQueries({ queryKey: messagesKey })
        // The backend resets each agent's last-known context usage on clear;
        // refetch so the avatar ring/tooltip drop the stale baseline immediately.
        if (scope === 'groups') {
          void qc.invalidateQueries({ queryKey: ['groups', conversationId, 'agents'] })
        }
      }
    },
  })
}

export function useClearGroupMessages(groupId: string | undefined) {
  return useClearConversationMessages('groups', groupId)
}

function useResetConversationContext(scope: ConversationScope, conversationId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () =>
      fetchJson<void>(`${conversationApiPath(scope, conversationId)}/context/reset`, {
        method: 'POST',
        token,
      }),
    onSuccess: () => {
      if (scope === 'groups') {
        void qc.invalidateQueries({ queryKey: ['groups', conversationId, 'agents'] })
      }
    },
  })
}

export function useResetDirectChatContext(chatId: string) {
  return useResetConversationContext('direct-chats', chatId)
}

export function useDeleteGroupMessage(groupId: string) {
  return useDeleteConversationMessage('groups', groupId)
}

export function useDeleteConversationMessage(scope: ConversationScope, groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  const removeMessage = useMessageStore((s) => s.removeMessage)
  return useMutation({
    mutationFn: ({ messageId }: { messageId: string }) =>
      fetchJson<void>(`${conversationApiPath(scope, groupId)}/messages/${messageId}`, {
        method: 'DELETE',
        token,
      }),
    onSuccess: (_, variables) => {
      qc.setQueryData<InfiniteData<Message[], string | undefined>>(
        conversationMessagesKey(scope, groupId),
        (current) => removeMessageFromPages(current, variables.messageId),
      )
      removeMessage(groupId, variables.messageId)
      void qc.invalidateQueries({ queryKey: conversationMessagesKey(scope, groupId) })
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
  return useConversationMessages('groups', groupId)
}

/** Fetch historical messages for either supported conversation container. */
export function useConversationMessages(
  scope: ConversationScope,
  groupId: string | undefined,
  threadId?: string,
) {
  const token = useAuthStore((s) => s.token)
  const setHistory = useMessageStore((s) => s.setHistory)

  const query = useInfiniteQuery({
    queryKey: conversationMessagesKey(scope, groupId, threadId),
    queryFn: ({ pageParam }: { pageParam?: string }) => {
      const params = new URLSearchParams({ limit: String(MESSAGE_PAGE_SIZE) })
      if (pageParam) params.set('before', pageParam)
      if (threadId) params.set('thread_id', threadId)
      return fetchJson<Message[]>(
        `${conversationApiPath(scope, groupId)}/messages?${params.toString()}`,
        { token },
      )
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
  }, [groupId, messages, query.data, setHistory, threadId])

  return query
}
