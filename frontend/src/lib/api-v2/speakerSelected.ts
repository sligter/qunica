import { z } from 'zod'

export const schedulerActionKindSchema = z.enum(['speak', 'call', 'handoff', 'wait', 'silent'])

export const schedulerSelectionReasonSchema = z.enum([
  'user_mention',
  'agent_call',
  'agent_handoff',
  'agent_text_mention',
  'deterministic_order',
  'moderator',
  'moderator_fallback',
  'decision_model',
])

// Required nullable source preserves the existing wire contract.
export const speakerSelectedPayloadSchema = z
  .object({
    turn_id: z.string(),
    dispatch_id: z.string(),
    source_agent_id: z.string().nullable(),
    target_agent_id: z.string(),
    reason: schedulerSelectionReasonSchema,
    action_kind: schedulerActionKindSchema,
    hop: z.number().int().nonnegative(),
  })
  .strict()

export type SchedulerActionKind = z.infer<typeof schedulerActionKindSchema>
export type SchedulerSelectionReason = z.infer<typeof schedulerSelectionReasonSchema>
export type SpeakerSelectedPayload = z.infer<typeof speakerSelectedPayloadSchema>
