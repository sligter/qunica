import { useMutation } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  AgentSystemPromptGenerateRequest,
  AgentSystemPromptGenerateResponse,
} from '@/types/api'

export function useGenerateAgentSystemPrompt() {
  const token = useAuthStore((state) => state.token)
  return useMutation({
    mutationFn: (body: AgentSystemPromptGenerateRequest) =>
      fetchJson<AgentSystemPromptGenerateResponse>('/agents/system-prompt/generate', {
        method: 'POST',
        token,
        body,
      }),
  })
}
