import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { GroupTemplateRead } from '@/types/api'

const groupTemplatesKey = ['group-templates'] as const

export function useGroupTemplates() {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: groupTemplatesKey,
    queryFn: () => fetchJson<GroupTemplateRead[]>('/group-templates', { token }),
    enabled: token !== null,
  })
}

export function useCreateGroupTemplate() {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: { name: string; group_id: string }) =>
      fetchJson<GroupTemplateRead>('/group-templates', { token, method: 'POST', body }),
    onSuccess: (created) => {
      queryClient.setQueryData<GroupTemplateRead[]>(groupTemplatesKey, (current = []) => [
        created,
        ...current.filter((template) => template.id !== created.id),
      ])
    },
  })
}

export function useDeleteGroupTemplate() {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (templateId: string) =>
      fetchJson<void>(`/group-templates/${templateId}`, { token, method: 'DELETE' }),
    onSuccess: (_result, templateId) => {
      queryClient.setQueryData<GroupTemplateRead[]>(groupTemplatesKey, (current = []) =>
        current.filter((template) => template.id !== templateId),
      )
    },
  })
}
