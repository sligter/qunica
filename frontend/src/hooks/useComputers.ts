import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { ComputerItem, ComputerList, ComputerState } from '@/types/api'

export function useComputers(enabled = true) {
  const token = useAuthStore(s => s.token)
  return useQuery({
    queryKey: ['computers'],
    queryFn: () => fetchJson<ComputerList>('/computers', { token }),
    enabled: enabled && !!token,
  })
}

export function useComputerState(id: string | undefined) {
  const token = useAuthStore(s => s.token)
  return useQuery({
    queryKey: ['computers', id],
    queryFn: () => fetchJson<ComputerState>(`/computers/${id}`, { token }),
    enabled: !!id && !!token,
    refetchInterval: 5_000,
  })
}

export function useComputerAction(item: ComputerItem, generation?: string, purgeHome = false) {
  const token = useAuthStore(s => s.token)
  const queries = useQueryClient()
  const { t } = useTranslation('computers')
  return useMutation({
    mutationFn: async (action: 'start' | 'stop' | 'rebuild' | 'destroy' | 'bind' | 'unbind') => {
      if (action === 'bind' || action === 'unbind') {
        return fetchJson('/execution-environments/binding', {
          method: 'PUT', token,
          body: { scope: item.binding.kind === 'team' ? 'group' : 'agent', target_id: item.binding.id, execution_environment_id: action === 'bind' ? item.id : null },
        })
      }
      if (!generation) throw new Error(t('refreshRequired'))
      return fetchJson(`/computers/${item.id}/actions`, {
        method: 'POST', token,
        body: { action, expected_generation: generation, confirm: action === 'destroy' ? item.id : null, purge_home: action === 'destroy' && purgeHome },
      })
    },
    onSettled: () => {
      for (const key of ['computers', 'groups', 'agents', 'direct-chats']) {
        void queries.invalidateQueries({ queryKey: [key] })
      }
    },
  })
}
