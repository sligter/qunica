import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { GroupThread } from '@/types/api'

export const groupThreadsKey = (groupId: string | undefined) =>
  ['groups', groupId, 'threads'] as const

export function useGroupThreads(groupId: string | undefined) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: groupThreadsKey(groupId),
    queryFn: () => fetchJson<GroupThread[]>(`/groups/${groupId}/threads`, { token }),
    enabled: token !== null && groupId !== undefined,
  })
}

export function useCreateGroupThread(groupId: string) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: { title: string; git_branch?: string | null }) =>
      fetchJson<GroupThread>(`/groups/${groupId}/threads`, {
        method: 'POST',
        token,
        body,
      }),
    onSuccess: (created) => {
      queryClient.setQueryData<GroupThread[]>(groupThreadsKey(groupId), (threads = []) => [
        created,
        ...threads.filter((thread) => thread.id !== created.id),
      ])
    },
  })
}

export function useArchiveGroupThread(groupId: string) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (threadId: string) =>
      fetchJson<GroupThread>(`/threads/${threadId}/archive`, {
        method: 'POST',
        token,
      }),
    onSuccess: (archived) => {
      queryClient.setQueryData<GroupThread[]>(groupThreadsKey(groupId), (threads = []) =>
        threads.map((thread) => thread.id === archived.id ? archived : thread),
      )
    },
  })
}

export function useRestoreGroupThread(groupId: string) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (threadId: string) =>
      fetchJson<GroupThread>(`/threads/${threadId}/unarchive`, {
        method: 'POST',
        token,
      }),
    onSuccess: (restored) => {
      queryClient.setQueryData<GroupThread[]>(groupThreadsKey(groupId), (threads = []) =>
        threads.map((thread) => thread.id === restored.id ? restored : thread),
      )
    },
  })
}

export function useDeleteGroupThread(groupId: string) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (threadId: string) =>
      fetchJson<void>(`/threads/${threadId}`, {
        method: 'DELETE',
        token,
      }),
    onSuccess: (_result, threadId) => {
      queryClient.setQueryData<GroupThread[]>(groupThreadsKey(groupId), (threads = []) =>
        threads.filter((thread) => thread.id !== threadId),
      )
    },
  })
}
