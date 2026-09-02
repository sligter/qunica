import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'

export function useAuthConfig() {
  return useQuery({
    queryKey: ['auth-config'],
    queryFn: () => fetchJson<{ registration_enabled: boolean }>('/auth/config'),
    staleTime: Infinity,
    retry: false,
  })
}
