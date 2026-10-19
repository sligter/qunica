import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Navigate, Outlet, useLocation } from 'react-router-dom'

import { fetchJson, ApiError } from '@/lib/api-v2/client'
import { expireAuthToken } from '@/lib/authFetch'
import { useAuthStore } from '@/stores/authStore'
import { startAccountSync } from '@/lib/realtime/accountSync'
import type { UserRead } from '@/types/api'
import { RealtimeStatus } from './RealtimeStatus'

export function RequireAuth() {
  const token = useAuthStore((s) => s.token)
  const setUser = useAuthStore((s) => s.setUser)
  const setHydrated = useAuthStore((s) => s.setHydrated)
  const hydrated = useAuthStore((s) => s.hydrated)
  const location = useLocation()
  const queries = useQueryClient()
  const meQuery = useQuery({
    queryKey: ['auth', 'me', token],
    queryFn: () => fetchJson<UserRead>('/auth/me', { token }),
    enabled: token !== null,
    retry: false,
  })
  const identityId = meQuery.data?.id
  useEffect(() => {
    if (token && identityId) return startAccountSync(token, queries)
  }, [token, queries, identityId])

  useEffect(() => {
    if (meQuery.data) {
      setUser(meQuery.data)
      setHydrated(true)
    }
    if (meQuery.error) {
      if (meQuery.error instanceof ApiError && meQuery.error.status === 401) {
        expireAuthToken(token, meQuery.error.code === 'session_upgrade_required'
          || meQuery.error.code === 'session_revoked' ? meQuery.error.code : null)
      }
      setHydrated(true)
    }
    if (token === null) {
      setHydrated(true)
    }
  }, [meQuery.data, meQuery.error, setUser, setHydrated, token])

  if (token === null) {
    return <Navigate to="/login" state={{ from: location }} replace />
  }

  if (!hydrated || meQuery.isLoading) {
    return (
      <div className="flex h-screen items-center justify-center text-muted-foreground">
        Opening workspace...
      </div>
    )
  }

  return <><RealtimeStatus /><Outlet /></>
}
