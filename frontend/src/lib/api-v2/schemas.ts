import { z } from 'zod'

import type { GroupSchedulerConfig } from './types'

export const groupSchedulerConfigSchema: z.ZodType<GroupSchedulerConfig> = z.object({
  scheduler_enabled: z.boolean(),
  agent_mention_policy: z.enum(['display_only', 'bounded_schedule']),
  max_agent_steps: z.number().int().min(1).nullable(),
  max_steps_per_agent: z.number().int().min(1),
  max_scheduler_hops: z.number().int().min(0),
  max_moderator_calls: z.number().int().min(0),
  max_consecutive_failures: z.number().int().min(1),
  max_total_failures: z.number().int().min(1),
  max_total_tokens: z.number().int().min(1),
  turn_timeout_seconds: z.number().int().min(1).max(3600),
  moderator_enabled: z.boolean(),
  moderator_provider_id: z.string().nullable(),
  moderator_model: z.string().nullable(),
})
