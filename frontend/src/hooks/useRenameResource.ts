import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'

/** Name-only list action shared by the five resource collections. */
export function useRenameResource(collectionPath: string, queryKey: readonly unknown[]) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      fetchJson(`${collectionPath}/${id}`, {
        token,
        method: 'PATCH',
        body: { name },
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey })
    },
  })
}
