import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { TokenUsageRead } from '@/types/api'

export interface TokenUsageFilters {
  from: string
  to: string
  group_id?: string
  provider_id?: string
  model?: string
  agent_id?: string
}

export function useTokenUsage(filters: TokenUsageFilters, enabled = true) {
  const token = useAuthStore((state) => state.token)
  const query = new URLSearchParams()
  Object.entries(filters).forEach(([key, value]) => {
    if (value) query.set(key, value)
  })

  return useQuery({
    queryKey: ['token-usage', filters],
    queryFn: () => fetchJson<TokenUsageRead>(`/token-usage?${query}`, { token }),
    enabled: enabled && token !== null,
    placeholderData: (previous) => previous,
  })
}
