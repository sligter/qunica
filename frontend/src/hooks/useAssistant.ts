import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { AssistantRead } from '@/types/api'

export const assistantQueryKey = ['assistant'] as const

/**
 * The built-in Assistant, created lazily on first read.
 *
 * `enabled` is gated on the token so the dock does not provision an Assistant
 * for a signed-out visitor sitting on the login screen.
 */
export function useAssistant() {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: assistantQueryKey,
    queryFn: () => fetchJson<AssistantRead>('/assistant', { token }),
    enabled: token !== null,
    // The row is stable for the life of the account; only the provider binding
    // changes, and that goes through the mutation below.
    staleTime: 5 * 60 * 1000,
  })
}

export function useBindAssistantProvider() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (llmProviderId: string | null) =>
      fetchJson<AssistantRead>('/assistant', {
        method: 'PATCH',
        token,
        body: { llm_provider_id: llmProviderId },
      }),
    onSuccess: (assistant) => {
      qc.setQueryData(assistantQueryKey, assistant)
    },
  })
}
