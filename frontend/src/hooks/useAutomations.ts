import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  AutomationConfigInput,
  AutomationHistory,
  AutomationList,
  AutomationPreview,
  AutomationPreviewInput,
  AutomationRead,
  AutomationSecretResponse,
  AutomationToggleInput,
} from '@/types/api'

export const automationsQueryKey = ['automations'] as const

export function useAutomations(options: { groupId?: string } = {}) {
  const token = useAuthStore((state) => state.token)
  const { groupId } = options
  return useQuery({
    queryKey: [...automationsQueryKey, groupId ?? null],
    queryFn: () => {
      const search = new URLSearchParams()
      if (groupId) search.set('group_id', groupId)
      const suffix = search.toString() ? `?${search.toString()}` : ''
      return fetchJson<AutomationList>(`/automations${suffix}`, { token })
    },
    enabled: token !== null,
    // Another device can pause or edit a task; the settings page is long-lived.
    refetchInterval: 15_000,
  })
}

export function useAutomation(id: string | null) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: [...automationsQueryKey, id],
    queryFn: () => fetchJson<AutomationRead>(`/automations/${id}`, { token }),
    enabled: token !== null && id !== null,
  })
}

export function usePreviewAutomation() {
  const token = useAuthStore((state) => state.token)
  return useMutation({
    mutationFn: (input: AutomationPreviewInput) =>
      fetchJson<AutomationPreview>('/automations/preview', {
        method: 'POST',
        token,
        body: input,
      }),
  })
}

export function useCreateAutomation() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: AutomationConfigInput) =>
      fetchJson<AutomationRead>('/automations', {
        method: 'POST',
        token,
        body: input,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: automationsQueryKey })
    },
  })
}

export function useUpdateAutomation() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, config }: { id: string; config: AutomationConfigInput }) =>
      fetchJson<AutomationRead>(`/automations/${id}`, {
        method: 'PATCH',
        token,
        body: config,
      }),
    onSuccess: (automation) => {
      qc.setQueryData([...automationsQueryKey, automation.id], automation)
      void qc.invalidateQueries({ queryKey: automationsQueryKey })
    },
  })
}

export function useToggleAutomation() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: AutomationToggleInput }) =>
      fetchJson<AutomationRead>(`/automations/${id}/toggle`, {
        method: 'POST',
        token,
        body: input,
      }),
    onSuccess: (automation) => {
      qc.setQueryData([...automationsQueryKey, automation.id], automation)
      void qc.invalidateQueries({ queryKey: automationsQueryKey })
    },
  })
}

export function useAutomationHistory(id: string | null, enabled: boolean) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: [...automationsQueryKey, id, 'history'],
    queryFn: () => fetchJson<AutomationHistory>(`/automations/${id}/history`, { token }),
    enabled: token !== null && id !== null && enabled,
  })
}

export function useAutomationSecret() {
  const token = useAuthStore((state) => state.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      id,
      action,
      expectedVersion,
    }: {
      id: string
      action: 'rotate' | 'revoke'
      expectedVersion: number
    }) =>
      fetchJson<AutomationSecretResponse>(`/automations/${id}/secret`, {
        method: 'POST',
        token,
        body: { action, expected_version: expectedVersion },
      }),
    onSuccess: (_result, variables) => {
      void qc.invalidateQueries({ queryKey: automationsQueryKey })
      void qc.invalidateQueries({ queryKey: [...automationsQueryKey, variables.id] })
    },
  })
}
