import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { GroupCreate, GroupRead } from '@/types/api'

export function useCreateGroup() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: (input: GroupCreate) =>
      fetchJson<GroupRead>('/groups', {
        method: 'POST',
        body: {
          name: input.name,
          ...(input.workspace_id ? { workspace_id: input.workspace_id } : {}),
          description: input.description,
          announcement: input.announcement,
          communication_mode: input.communication_mode,
          initial_agents: input.initial_agents,
        },
        token,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
