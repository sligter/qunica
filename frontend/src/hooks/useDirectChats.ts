import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { DirectChatCreate, DirectChatRead, DirectChatUpdate } from '@/types/api'

export const directChatsQueryKey = ['direct-chats'] as const
export const directChatQueryKey = (chatId: string | undefined) =>
  ['direct-chats', chatId] as const

export function sortDirectChatsByActivity(chats: DirectChatRead[]): DirectChatRead[] {
  return [...chats].sort((left, right) => right.updated_at.localeCompare(left.updated_at))
}

export function replaceDirectChatInList(
  chats: DirectChatRead[] | undefined,
  chat: DirectChatRead,
): DirectChatRead[] {
  const existing = chats ?? []
  const index = existing.findIndex((item) => item.id === chat.id)
  const next = index === -1
    ? [...existing, chat]
    : existing.map((item) => (item.id === chat.id ? chat : item))
  return sortDirectChatsByActivity(next)
}

export function useDirectChats() {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: directChatsQueryKey,
    queryFn: () => fetchJson<DirectChatRead[]>('/direct-chats', { token }),
    enabled: token !== null,
  })
}

export function useDirectChat(chatId: string | undefined) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: directChatQueryKey(chatId),
    queryFn: () => fetchJson<DirectChatRead>(`/direct-chats/${chatId}`, { token }),
    enabled: token !== null && chatId !== undefined,
  })
}

export function useCreateDirectChat() {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (input: DirectChatCreate) =>
      fetchJson<DirectChatRead>('/direct-chats', { method: 'POST', body: input, token }),
    onSuccess: (chat) => {
      queryClient.setQueryData<DirectChatRead[]>(directChatsQueryKey, (current) =>
        replaceDirectChatInList(current, chat),
      )
      queryClient.setQueryData(directChatQueryKey(chat.id), chat)
    },
  })
}

interface RenameContext {
  list: DirectChatRead[] | undefined
  detail: DirectChatRead | undefined
}

export function useRenameDirectChat(chatId: string) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (input: DirectChatUpdate) =>
      fetchJson<DirectChatRead>(`/direct-chats/${chatId}`, {
        method: 'PATCH',
        body: input,
        token,
      }),
    onMutate: async (input): Promise<RenameContext> => {
      await Promise.all([
        queryClient.cancelQueries({ queryKey: directChatsQueryKey }),
        queryClient.cancelQueries({ queryKey: directChatQueryKey(chatId) }),
      ])
      const list = queryClient.getQueryData<DirectChatRead[]>(directChatsQueryKey)
      const detail = queryClient.getQueryData<DirectChatRead>(directChatQueryKey(chatId))
      const optimistic = detail
        ? { ...detail, title: input.title.trim(), title_source: 'manual' as const }
        : undefined
      if (optimistic) {
        queryClient.setQueryData(directChatQueryKey(chatId), optimistic)
        queryClient.setQueryData<DirectChatRead[]>(directChatsQueryKey, (current) =>
          replaceDirectChatInList(current, optimistic),
        )
      } else if (list) {
        queryClient.setQueryData(
          directChatsQueryKey,
          list.map((chat) =>
            chat.id === chatId
              ? { ...chat, title: input.title.trim(), title_source: 'manual' as const }
              : chat,
          ),
        )
      }
      return { list, detail }
    },
    onError: (_error, _input, context) => {
      if (context) {
        queryClient.setQueryData(directChatsQueryKey, context.list)
        queryClient.setQueryData(directChatQueryKey(chatId), context.detail)
      }
    },
    onSuccess: (chat) => {
      queryClient.setQueryData(directChatQueryKey(chatId), chat)
      queryClient.setQueryData<DirectChatRead[]>(directChatsQueryKey, (current) =>
        replaceDirectChatInList(current, chat),
      )
    },
  })
}

export function useDeleteDirectChat(chatId: string) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => fetchJson<void>(`/direct-chats/${chatId}`, { method: 'DELETE', token }),
    onSuccess: () => {
      queryClient.setQueryData<DirectChatRead[]>(directChatsQueryKey, (current) =>
        current?.filter((chat) => chat.id !== chatId) ?? [],
      )
      queryClient.removeQueries({ queryKey: directChatQueryKey(chatId) })
    },
  })
}
