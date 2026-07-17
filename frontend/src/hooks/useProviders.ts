import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  LLMProviderCreate,
  LLMProviderRead,
  LLMProviderUpdate,
  ModelInfo,
} from '@/types/api'

export function useProviders() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['llm-providers'],
    queryFn: () => fetchJson<LLMProviderRead[]>('/llm-providers', { token }),
    enabled: token !== null,
  })
}

export function useProvider(id: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['llm-providers', id],
    queryFn: () => fetchJson<LLMProviderRead>(`/llm-providers/${id}`, { token }),
    enabled: token !== null && !!id,
  })
}

export function useCreateProvider() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: LLMProviderCreate) =>
      fetchJson<LLMProviderRead>('/llm-providers', {
        token,
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['llm-providers'] })
    },
  })
}

export function useUpdateProvider(providerId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: LLMProviderUpdate) =>
      fetchJson<LLMProviderRead>(`/llm-providers/${providerId}`, {
        token,
        method: 'PATCH',
        body: data,
      }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['llm-providers'] })
      qc.setQueryData(['llm-providers', providerId], updated)
    },
  })
}

export function useDeleteProvider() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      fetchJson<void>(`/llm-providers/${id}`, {
        token,
        method: 'DELETE',
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['llm-providers'] })
    },
  })
}

export function useProviderModels(providerId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['llm-providers', providerId, 'models'],
    queryFn: () =>
      fetchJson<ModelInfo[]>(`/llm-providers/${providerId}/models`, {
        token,
      }),
    enabled: token !== null && !!providerId,
    staleTime: 5 * 60 * 1000,
  })
}
