import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupNoteCreate, GroupNoteRead, GroupNoteUpdate } from '@/types/api'

export function useGroupNotes(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'notes'],
    queryFn: () => fetchJson<GroupNoteRead[]>(`/groups/${groupId}/notes`, { token }),
    enabled: token !== null && !!groupId,
  })
}

export function useCreateGroupNote(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: GroupNoteCreate) =>
      fetchJson<GroupNoteRead>(`/groups/${groupId}/notes`, {
        token,
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'notes'] })
    },
  })
}

export function useUpdateGroupNote(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ noteId, data }: { noteId: string; data: GroupNoteUpdate }) =>
      fetchJson<GroupNoteRead>(`/groups/${groupId}/notes/${noteId}`, {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'notes'] })
    },
  })
}

export function useDeleteGroupNote(groupId: string) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (noteId: string) =>
      fetchJson<void>(`/groups/${groupId}/notes/${noteId}`, { token, method: 'DELETE' }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'notes'] })
    },
  })
}
