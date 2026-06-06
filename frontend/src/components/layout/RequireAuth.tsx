import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Navigate, Outlet, useLocation } from 'react-router-dom'

import { fetchJson, ApiError } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { UserRead } from '@/types/api'

export function RequireAuth() {
  const token = useAuthStore((s) => s.token)
  const setUser = useAuthStore((s) => s.setUser)
  const setHydrated = useAuthStore((s) => s.setHydrated)
  const logout = useAuthStore((s) => s.logout)
  const hydrated = useAuthStore((s) => s.hydrated)
  const location = useLocation()

  const meQuery = useQuery({
    queryKey: ['auth', 'me', token],
    queryFn: () => fetchJson<UserRead>('/auth/me', { token }),
    enabled: token !== null,
    retry: false,
  })

  useEffect(() => {
    if (meQuery.data) {
      setUser(meQuery.data)
      setHydrated(true)
    }
    if (meQuery.error) {
      if (meQuery.error instanceof ApiError && meQuery.error.status === 401) {
        logout()
      }
      setHydrated(true)
    }
    if (token === null) {
      setHydrated(true)
    }
  }, [meQuery.data, meQuery.error, setUser, setHydrated, logout, token])

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

  return <Outlet />
}
