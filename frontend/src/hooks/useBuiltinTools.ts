import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { ToolCatalogResponse } from '@/types/api'

export function useBuiltinTools() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['agents', 'tool-catalog'],
    queryFn: () => fetchJson<ToolCatalogResponse>('/agents/tool-catalog', { token }),
    enabled: token !== null,
    staleTime: 5 * 60 * 1000,
  })
}
