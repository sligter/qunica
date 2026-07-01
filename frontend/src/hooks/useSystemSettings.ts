import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { SystemSettingsRead, SystemSettingsUpdate } from '@/types/api'

export function useSystemSettings() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['settings', 'system'],
    queryFn: () => fetchJson<SystemSettingsRead>('/settings/system', { token }),
    enabled: token !== null,
  })
}

export function useUpdateSystemSettings() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: SystemSettingsUpdate) =>
      fetchJson<SystemSettingsRead>('/settings/system', {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: (updated) => {
      qc.setQueryData(['settings', 'system'], updated)
    },
  })
}
