import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  AcpRuntimeInstallResponse,
  AcpRuntimePresetRead,
  AcpRuntimeVersionListResponse,
} from '@/types/api'

const VERSION_QUERY_KEY = ['agents', 'acp-runtime-versions'] as const

export function useAcpRuntimeVersions() {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: VERSION_QUERY_KEY,
    queryFn: () =>
      fetchJson<AcpRuntimeVersionListResponse>('/agents/acp-runtime-versions', { token }),
    enabled: token !== null,
    staleTime: 60_000,
  })
}

export function useInstallAcpRuntimeVersion() {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({
      presetId,
      packageSpec,
    }: {
      presetId: AcpRuntimePresetRead['id']
      packageSpec?: string
    }) =>
      fetchJson<AcpRuntimeInstallResponse>(
        `/agents/acp-runtime-versions/${presetId}/install`,
        {
          method: 'POST',
          token,
          body: packageSpec ? { package_spec: packageSpec } : {},
        },
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: VERSION_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: ['agents', 'acp-runtime-presets'] }),
      ])
    },
  })
}
