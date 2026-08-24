import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { UserRead, UserUpdate } from '@/types/api'

export function useAuth() {
  const token = useAuthStore((s) => s.token)
  const user = useAuthStore((s) => s.user)
  const hydrated = useAuthStore((s) => s.hydrated)
  const setToken = useAuthStore((s) => s.setToken)
  const setUser = useAuthStore((s) => s.setUser)
  const logout = useAuthStore((s) => s.logout)
  return { token, user, hydrated, setToken, setUser, logout }
}

export function useUpdateCurrentUser() {
  const token = useAuthStore((s) => s.token)
  const setUser = useAuthStore((s) => s.setUser)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: UserUpdate) =>
      fetchJson<UserRead>('/auth/me', { token, method: 'PATCH', body: data }),
    onMutate: (data) => {
      const previous = useAuthStore.getState().user
      if (previous) {
        const optimistic = { ...previous, ...data }
        useAuthStore.setState({ user: optimistic })
        qc.setQueryData(['auth', 'me', token], optimistic)
      }
      return { previous }
    },
    onError: (_error, _data, context) => {
      if (context?.previous) {
        useAuthStore.setState({ user: context.previous })
        qc.setQueryData(['auth', 'me', token], context.previous)
      }
    },
    onSuccess: (updated) => {
      setUser(updated)
      qc.setQueryData(['auth', 'me', token], updated)
      void qc.invalidateQueries({
        predicate: ({ queryKey }) => queryKey[0] === 'groups' && queryKey[2] === 'members',
      })
    },
  })
}
