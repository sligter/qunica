import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { ExternalAdapterStatusResponse } from '@/types/api'

export function useExternalRuntimes() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['external-runtimes'],
    queryFn: () => fetchJson<ExternalAdapterStatusResponse>('/agents/external-runtimes/status', { token }),
    enabled: Boolean(token),
    staleTime: 30_000,
  })
}
