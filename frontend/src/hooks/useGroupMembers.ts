import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/http'
import { useAuthStore } from '@/stores/authStore'
import type { GroupMemberRead, UserRead } from '@/types/api'

interface MemberMutationVars {
  groupId: string
  userId: string
}

interface MemberMuteVars extends MemberMutationVars {
  muted: boolean
}

export function useGroupMembers(groupId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'members'],
    queryFn: () => fetchJson<GroupMemberRead[]>(`/groups/${groupId}/members`, { token }),
    enabled: token !== null && groupId !== undefined,
  })
}

export function useGroupMemberCandidates(groupId: string | undefined, query: string) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['groups', groupId, 'member-candidates', query],
    queryFn: () =>
      fetchJson<UserRead[]>(
        `/groups/${groupId}/member-candidates?q=${encodeURIComponent(query)}`,
        { token },
      ),
    enabled: token !== null && groupId !== undefined,
  })
}

export function useAddGroupMember() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, userId }: MemberMutationVars) =>
      fetchJson<GroupMemberRead>(`/groups/${groupId}/members`, {
        token,
        method: 'POST',
        body: { user_id: userId },
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'members'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'member-candidates'] })
    },
  })
}

export function useRemoveGroupMember() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, userId }: MemberMutationVars) =>
      fetchJson<void>(`/groups/${groupId}/members/${userId}`, {
        token,
        method: 'DELETE',
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'members'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'member-candidates'] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}

export function useMuteGroupMember() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, userId, muted }: MemberMuteVars) =>
      fetchJson<GroupMemberRead>(`/groups/${groupId}/members/${userId}/mute`, {
        token,
        method: 'PATCH',
        body: { muted },
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'members'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
