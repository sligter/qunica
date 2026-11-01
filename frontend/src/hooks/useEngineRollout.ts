import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'

export interface EngineCapabilities {
  rig_available: boolean
  rig_paused: boolean
  default_engine: 'native'
  app_version: string
  rig_version: string
}
export interface EngineMetricsRow {
  engine: 'native' | 'rig'
  app_version: string
  engine_version: string
  provider_id: string | null
  provider_kind: string
  model: string
  executions: number
  partial_executions: number
  completed: number
  failed: number
  cancelled: number
  interrupted: number
  pending: number
  blocked: number
  outcome_unknown: number
  unsettled_tools: number
  tool_calls: number
  settled_tool_calls: number
  tool_errors: number
  total_tokens: number
  usage_calls: number
  executions_without_usage: number
  active_duration_ms: number
  failure_rate: number | null
  tool_error_rate: number | null
}
export interface EngineMetricsResponse {
  days: number
  rows: EngineMetricsRow[]
  unattributed_calls: number
}

export function useEngineCapabilities() {
  const token = useAuthStore(state => state.token)
  return useQuery({
    queryKey: ['runtime-engines', token],
    queryFn: () => fetchJson<EngineCapabilities>('/runtime/engines', { token }),
    enabled: token !== null,
    retry: false,
  })
}

export function usePauseRig() {
  const token = useAuthStore(state => state.token)
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (paused: boolean) => ({
      token,
      data: await fetchJson<EngineCapabilities>('/runtime/engines/rig', {
        token, method: 'PATCH', body: { paused },
      }),
    }),
    // Note: the account can change while PATCH is in flight. Its response
    // belongs to the request's account, never the newly logged-in account.
    onSuccess: result => client.setQueryData(['runtime-engines', result.token], result.data),
  })
}

export function useEngineMetrics(days: number) {
  const token = useAuthStore(state => state.token)
  return useQuery({
    queryKey: ['engine-metrics', token, days],
    queryFn: () => fetchJson<EngineMetricsResponse>(`/runtime/engine-metrics?days=${days}`, { token }),
    enabled: token !== null,
    retry: false,
  })
}
