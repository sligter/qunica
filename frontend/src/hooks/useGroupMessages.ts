import { useCallback, useLayoutEffect, useMemo } from 'react'
import {
  infiniteQueryOptions,
  useInfiniteQuery,
  useMutation,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
} from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { groupThreadsQueryOptions } from '@/hooks/useGroupThreads'
import { useAuthStore } from '@/stores/authStore'
import { useMessageStore } from '@/stores/messageStore'
import type {
  ClearGroupMessagesResponse,
  GroupPromptEnhanceRequest,
  GroupPromptEnhanceResponse,
  GroupThread,
  Message,
  MessageAttachment,
  MessageSendResponse,
} from '@/types/api'

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

type ConversationMessagesKey = ReturnType<typeof conversationMessagesKey>

export const conversationStateKey = (
  conversationId: string | undefined,
  threadId?: string | null,
) => threadId ?? conversationId

export const conversationApiPath = (scope: ConversationScope, id: string | undefined) =>
  `/${scope}/${id}`

function hasLocalStream(stateId: string | undefined): boolean {
  if (!stateId) return false
  const state = useMessageStore.getState()
  return Boolean(state.activeSendsByGroup[stateId]) || Object.values(state.activeResumesByMessageId)
    .some(resume => resume.state_id === stateId)
}

export const conversationMessagesQueryOptions = (
  scope: ConversationScope,
  conversationId: string | undefined,
  token: string | null,
  threadId?: string,
) => infiniteQueryOptions<
  Message[],
  Error,
  InfiniteData<Message[], string | undefined>,
  ConversationMessagesKey,
  string | undefined
>({
  queryKey: conversationMessagesKey(scope, conversationId, threadId),
  queryFn: ({ pageParam }) => {
    const params = new URLSearchParams({ limit: String(MESSAGE_PAGE_SIZE) })
    if (pageParam) params.set('before', pageParam)
    if (threadId) params.set('thread_id', threadId)
    return fetchJson<Message[]>(
      `${conversationApiPath(scope, conversationId)}/messages?${params.toString()}`,
      { token },
    )
  },
  enabled: token !== null && conversationId !== undefined,
  // A reloaded phone has no local SSE owner. Refresh its server snapshot without
  // executing another send/resume. Live streams own their incremental state.
  refetchOnWindowFocus: () => !hasLocalStream(conversationStateKey(conversationId, threadId)),
  refetchOnReconnect: () => !hasLocalStream(conversationStateKey(conversationId, threadId)),
  refetchInterval: query => {
    if (hasLocalStream(conversationStateKey(conversationId, threadId))) return false
    return query.state.data?.pages.some(page => page.some(message =>
      message.turn_summary && ['pending', 'running'].includes(message.turn_summary.status),
    )) ? 2_000 : false
  },
  initialPageParam: INITIAL_PAGE_PARAM,
  getNextPageParam: (lastPage) =>
    lastPage.length === MESSAGE_PAGE_SIZE ? lastPage[0]?.id : undefined,
})

function preferredGroupThreadId(groupId: string, threads: GroupThread[]): string | undefined {
  let storedId: string | null = null
  try {
    storedId = window.localStorage.getItem(`qunica:groups:selected-thread:${groupId}`)
  } catch {
    // Storage availability must not block intent prefetching.
  }
  return threads.find((thread) => thread.id === storedId)?.id
    ?? threads.find((thread) => thread.status !== 'archived')?.id
    ?? threads[0]?.id
}

export function prefetchConversation(
  queryClient: QueryClient,
  token: string | null,
  scope: ConversationScope,
  conversationId: string,
  threadId?: string,
): Promise<void> | undefined {
  if (!token) return
  if (scope === 'direct-chats' || threadId) {
    return queryClient.prefetchInfiniteQuery(
      conversationMessagesQueryOptions(scope, conversationId, token, threadId),
    )
  }
  return queryClient.fetchQuery(groupThreadsQueryOptions(conversationId, token))
    .then((threads) => queryClient.prefetchInfiniteQuery(
      conversationMessagesQueryOptions(
        scope,
        conversationId,
        token,
        preferredGroupThreadId(conversationId, threads),
      ),
    ))
    .catch(() => undefined)
}

export function useConversationPrefetch() {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useCallback((scope: ConversationScope, conversationId: string, threadId?: string) => {
    void prefetchConversation(queryClient, token, scope, conversationId, threadId)
  }, [queryClient, token])
}

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

export function useClearGroupThreadMessages(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  const clearGroupMessages = useMessageStore((s) => s.clearGroupMessages)
  return useMutation({
    mutationFn: (threadId: string) =>
      fetchJson<ClearGroupMessagesResponse>(`/threads/${threadId}/messages/clear`, {
        method: 'POST',
        token,
      }),
    onSuccess: (_result, threadId) => {
      const messagesKey = conversationMessagesKey('groups', groupId, threadId)
      qc.setQueryData(messagesKey, emptyMessagePages())
      clearGroupMessages(threadId)
      void qc.invalidateQueries({ queryKey: messagesKey })
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
    },
  })
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

export function useDeleteConversationMessage(
  scope: ConversationScope,
  groupId: string,
  threadId?: string | null,
) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  const removeMessage = useMessageStore((s) => s.removeMessage)
  const stateKey = threadId ?? groupId
  return useMutation({
    mutationFn: ({ messageId }: { messageId: string }) =>
      fetchJson<void>(`${conversationApiPath(scope, groupId)}/messages/${messageId}`, {
        method: 'DELETE',
        token,
      }),
    onSuccess: (_, variables) => {
      qc.setQueryData<InfiniteData<Message[], string | undefined>>(
        conversationMessagesKey(scope, groupId, threadId ?? undefined),
        (current) => removeMessageFromPages(current, variables.messageId),
      )
      removeMessage(stateKey, variables.messageId)
      void qc.invalidateQueries({
        queryKey: conversationMessagesKey(scope, groupId, threadId ?? undefined),
      })
    },
  })
}

export interface SendGroupMessageVariables {
  groupId: string
  content: string
  /**
   * Workspace paths in the *target* group. A file from another conversation has
   * to be copied there first: the send endpoint validates every path against
   * the receiving group's workspace.
   */
  attachments?: Array<Pick<MessageAttachment, 'path'>>
}

export function useSendGroupMessage() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ groupId, content, attachments = [] }: SendGroupMessageVariables) =>
      fetchJson<MessageSendResponse>(`/groups/${groupId}/messages`, {
        method: 'POST',
        token,
        body: { content, attachments },
      }),
    onSuccess: (_, variables) => {
      void qc.invalidateQueries({ queryKey: ['groups', variables.groupId, 'messages'] })
    },
  })
}

export function useEnhanceGroupPrompt(groupId: string, threadId?: string) {
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: (body: Omit<GroupPromptEnhanceRequest, 'thread_id'>) =>
      fetchJson<GroupPromptEnhanceResponse>(`/groups/${groupId}/prompt/enhance`, {
        method: 'POST',
        token,
        body: {
          ...body,
          thread_id: threadId ?? null,
        } satisfies GroupPromptEnhanceRequest,
      }),
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
  const stateKey = conversationStateKey(groupId, threadId)

  const query = useInfiniteQuery(
    conversationMessagesQueryOptions(scope, groupId, token, threadId),
  )

  const messages = useMemo(
    () => [...(query.data?.pages ?? [])].reverse().flat(),
    [query.data?.pages],
  )

  useLayoutEffect(() => {
    if (stateKey && query.data) {
      setHistory(stateKey, messages)
    }
  }, [messages, query.data, setHistory, stateKey])

  return query
}
