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
  /** Minutes east of UTC the from/to dates are expressed in. */
  tz_offset_minutes?: number
}

/**
 * The viewer's UTC offset, so the backend buckets records by the days the user
 * actually lives in. Records are stored with UTC timestamps; without this a
 * UTC+8 user's "today" opened missing everything before 08:00 local.
 */
function localTzOffsetMinutes(): number {
  return -new Date().getTimezoneOffset()
}

export function useTokenUsage(filters: TokenUsageFilters, enabled = true) {
  const token = useAuthStore((state) => state.token)
  const query = new URLSearchParams()
  Object.entries(filters).forEach(([key, value]) => {
    if (value) query.set(key, value)
  })
  query.set('tz_offset_minutes', String(localTzOffsetMinutes()))

  return useQuery({
    queryKey: ['token-usage', filters],
    queryFn: () => fetchJson<TokenUsageRead>(`/token-usage?${query}`, { token }),
    enabled: enabled && token !== null,
    placeholderData: (previous) => previous,
  })
}
