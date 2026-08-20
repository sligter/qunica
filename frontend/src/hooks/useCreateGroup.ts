import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
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
          template_id: input.template_id,
          ...(input.workspace_id ? { workspace_id: input.workspace_id } : {}),
          description: input.description,
          announcement: input.announcement,
          auto_share_workspace_with_new_agents: input.auto_share_workspace_with_new_agents,
          free_speech: input.free_speech,
          proactive_mode: input.proactive_mode,
          allow_agent_free_mention: input.allow_agent_free_mention,
          agent_free_mention_max_dispatches: input.agent_free_mention_max_dispatches,
          communication_mode: input.communication_mode,
          initial_agents: input.initial_agents,
          scheduler_mode: input.scheduler_mode,
          agent_mention_policy: input.agent_mention_policy,
          max_agent_steps: input.max_agent_steps,
          max_steps_per_agent: input.max_steps_per_agent,
          max_scheduler_hops: input.max_scheduler_hops,
          max_moderator_calls: input.max_moderator_calls,
          max_consecutive_failures: input.max_consecutive_failures,
          max_total_failures: input.max_total_failures,
          max_total_tokens: input.max_total_tokens,
          turn_timeout_seconds: input.turn_timeout_seconds,
          moderator_enabled: input.moderator_enabled,
          moderator_provider_id: input.moderator_provider_id,
          moderator_model: input.moderator_model,
        },
        token,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
