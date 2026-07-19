import { z } from 'zod'

import type {
  GroupSchedulerConfig,
  GroupTurnTraceResponse,
  ConversationUpdatedPayload,
  SchedulerStreamEventKind,
  SchedulerStreamUpdate,
  StreamEvent,
  StreamEventKind,
} from './types'

const conversationUpdatedEventSchema = z
  .object({
    stream_id: z.string(),
    seq: z.number().int().nonnegative(),
    event_id: z.string(),
    kind: z.literal('conversation_updated'),
    payload: z
      .object({
        conversation_id: z.string(),
        title: z.string(),
        title_source: z.enum(['automatic', 'manual']),
        updated_at: z.string(),
      })
      .strict(),
  })
  .strict()

export function parseConversationUpdatedEvent(
  event: StreamEvent<unknown, StreamEventKind>,
): StreamEvent<ConversationUpdatedPayload, 'conversation_updated'> {
  return conversationUpdatedEventSchema.parse(event)
}

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

const groupTurnStatusSchema = z.enum([
  'pending',
  'running',
  'waiting_for_user',
  'completed',
  'silence',
  'budget_exhausted',
  'failure_budget_exhausted',
  'cancelled',
  'superseded',
  'failed',
])

const agentDispatchStatusSchema = z.enum([
  'queued',
  'running',
  'completed',
  'silent',
  'waiting_for_user',
  'interrupted',
  'cancelled',
  'failed',
])

const schedulerActionKindSchema = z.enum(['speak', 'call', 'handoff', 'wait', 'silent'])

const schedulerSelectionReasonSchema = z.enum([
  'user_mention',
  'agent_call',
  'agent_handoff',
  'agent_text_mention',
  'deterministic_order',
  'moderator',
  'moderator_fallback',
])

const groupTurnTerminationReasonSchema = z.enum([
  'waiting_for_user',
  'budget_exhausted',
  'failure_budget_exhausted',
  'user_cancelled',
  'superseded',
  'server_restart',
  'persistence_failed',
  'silence',
])

const groupTurnBudgetUsageSchema = z
  .object({
    agent_steps: z.number().int().nonnegative(),
    moderator_calls: z.number().int().nonnegative(),
    consecutive_failures: z.number().int().nonnegative(),
    total_failures: z.number().int().nonnegative(),
    total_tokens: z.number().int().nonnegative(),
  })
  .strict()

const groupTurnBudgetLimitsSchema = z
  .object({
    max_agent_steps: z.number().int().positive(),
    max_steps_per_agent: z.number().int().positive(),
    max_hops: z.number().int().nonnegative(),
    max_moderator_calls: z.number().int().nonnegative(),
    max_consecutive_failures: z.number().int().positive(),
    max_total_failures: z.number().int().positive(),
    max_total_tokens: z.number().int().positive(),
  })
  .strict()

const groupTurnTerminalBudgetSchema = groupTurnBudgetUsageSchema
  .extend({ limits: groupTurnBudgetLimitsSchema.optional() })
  .strict()

const schedulerEventBaseSchema = z
  .object({
    stream_id: z.string(),
    seq: z.number().int().nonnegative(),
    event_id: z.string(),
  })
  .strict()

const turnCancelledPayloadSchema = z
  .object({
    turn_id: z.string(),
    status: z.literal('cancelled'),
    reason: z.literal('user_cancelled'),
    budget: groupTurnTerminalBudgetSchema,
  })
  .strict()

const turnSupersededPayloadSchema = z
  .object({
    turn_id: z.string(),
    status: z.literal('superseded'),
    reason: z.literal('superseded'),
    budget: groupTurnTerminalBudgetSchema,
  })
  .strict()

const turnBudgetExhaustedPayloadSchema = z.union([
  z
    .object({
      turn_id: z.string(),
      status: z.literal('budget_exhausted'),
      reason: z.literal('budget_exhausted'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
  z
    .object({
      turn_id: z.string(),
      status: z.literal('failure_budget_exhausted'),
      reason: z.literal('failure_budget_exhausted'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
])

const turnCompletedPayloadSchema = z.union([
  z
    .object({
      turn_id: z.string(),
      status: z.literal('waiting_for_user'),
      reason: z.literal('waiting_for_user'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
  z
    .object({
      turn_id: z.string(),
      status: z.literal('completed'),
      reason: z.null(),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
  z
    .object({
      turn_id: z.string(),
      status: z.literal('silence'),
      reason: z.literal('silence'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
  z
    .object({
      turn_id: z.string(),
      status: z.literal('failed'),
      reason: z.literal('persistence_failed'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
  z
    .object({
      turn_id: z.string(),
      status: z.literal('budget_exhausted'),
      reason: z.literal('budget_exhausted'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
  z
    .object({
      turn_id: z.string(),
      status: z.literal('failure_budget_exhausted'),
      reason: z.literal('failure_budget_exhausted'),
      budget: groupTurnTerminalBudgetSchema,
    })
    .strict(),
])

const schedulerEventSchema: z.ZodType<SchedulerStreamUpdate> = z.discriminatedUnion('kind', [
  schedulerEventBaseSchema.extend({
    kind: z.literal('turn_started'),
    payload: z
      .object({
        turn_id: z.string(),
        budget: groupTurnBudgetLimitsSchema,
      })
      .strict(),
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('speaker_selected'),
    payload: z
      .object({
        turn_id: z.string(),
        dispatch_id: z.string(),
        source_agent_id: z.string().nullable(),
        target_agent_id: z.string(),
        reason: schedulerSelectionReasonSchema,
        action_kind: schedulerActionKindSchema,
        hop: z.number().int().nonnegative(),
      })
      .strict(),
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('dispatch_failed'),
    payload: z
      .object({
        turn_id: z.string(),
        dispatch_id: z.string(),
        target_agent_id: z.string(),
        action_kind: schedulerActionKindSchema,
        reason: z.literal('persistence_failed'),
      })
      .strict(),
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('moderator_fallback'),
    payload: z
      .object({
        turn_id: z.string(),
        dispatch_id: z.string(),
        target_agent_id: z.string(),
        reason: z.literal('moderator_fallback'),
      })
      .strict(),
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('turn_cancelled'),
    payload: turnCancelledPayloadSchema,
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('turn_superseded'),
    payload: turnSupersededPayloadSchema,
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('turn_budget_exhausted'),
    payload: turnBudgetExhaustedPayloadSchema,
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('turn_completed'),
    payload: turnCompletedPayloadSchema,
  }),
  schedulerEventBaseSchema.extend({
    kind: z.literal('done'),
    payload: z.object({ turn_id: z.string() }).strict(),
  }),
])

const publicTurnArtifactSchema = z
  .object({
    mode: z.enum(['call', 'handoff']).optional(),
    target_agent_id: z.string().optional(),
    child_dispatch_id: z.string().optional(),
    outcome: z.string().optional(),
    failure_code: z.string().optional(),
  })
  .strict()
  .refine((artifact) => Object.keys(artifact).length > 0, 'public artifact must not be empty')

const groupTurnSummarySchema = z
  .object({
    id: z.string(),
    thread_id: z.string(),
    group_id: z.string(),
    trigger_message_id: z.string().nullable(),
    status: groupTurnStatusSchema,
    scheduler_strategy: z.string(),
    config_snapshot: z.record(z.unknown()),
    topology_snapshot: z.record(z.unknown()),
    agent_steps: z.number().int().nonnegative(),
    moderator_calls: z.number().int().nonnegative(),
    consecutive_failures: z.number().int().nonnegative(),
    total_failures: z.number().int().nonnegative(),
    total_tokens: z.number().int().nonnegative(),
    termination_reason: groupTurnTerminationReasonSchema.nullable(),
    created_at: z.string(),
    started_at: z.string().nullable(),
    completed_at: z.string().nullable(),
    updated_at: z.string(),
  })
  .strict()

const agentDispatchTraceSchema = z
  .object({
    id: z.string(),
    turn_id: z.string(),
    parent_dispatch_id: z.string().nullable(),
    source_agent_id: z.string().nullable(),
    target_agent_id: z.string(),
    selection_reason: schedulerSelectionReasonSchema,
    action_kind: schedulerActionKindSchema,
    hop: z.number().int().nonnegative(),
    status: agentDispatchStatusSchema,
    input_message_id: z.string().nullable(),
    output_message_id: z.string().nullable(),
    artifact: publicTurnArtifactSchema.nullable(),
    total_tokens: z.number().int().nonnegative(),
    failure_code: z.string().nullable(),
    created_at: z.string(),
    started_at: z.string().nullable(),
    completed_at: z.string().nullable(),
    updated_at: z.string(),
  })
  .strict()

const estimatedCostSchema = z
  .object({
    amount: z.string().regex(/^-?(?:0|[1-9]\d*)(?:\.\d+)?$/),
    currency: z.string().min(1),
  })
  .strict()

const groupTurnTraceSchema: z.ZodType<GroupTurnTraceResponse> = z
  .object({
    turn: groupTurnSummarySchema,
    budget: groupTurnBudgetUsageSchema,
    dispatches: z.array(agentDispatchTraceSchema),
    estimated_cost: estimatedCostSchema.nullable(),
    cost_estimation_status: z.literal('unavailable'),
  })
  .strict()

type SchedulerParseEventKind = SchedulerStreamEventKind | 'done'

const schedulerStreamEventKinds = new Set<StreamEventKind>([
  'turn_started',
  'speaker_selected',
  'dispatch_failed',
  'moderator_fallback',
  'turn_cancelled',
  'turn_superseded',
  'turn_budget_exhausted',
  'turn_completed',
  'done',
])

function isSchedulerParseEventKind(kind: StreamEventKind): kind is SchedulerParseEventKind {
  return schedulerStreamEventKinds.has(kind)
}

function isLegacyDonePayload(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value).length === 0
  )
}

export function parseSchedulerStreamEvent(
  event: StreamEvent<unknown, StreamEventKind>,
): SchedulerStreamUpdate | null {
  if (!isSchedulerParseEventKind(event.kind)) {
    return null
  }
  if (event.kind === 'done' && isLegacyDonePayload(event.payload)) {
    return null
  }
  const parsed = schedulerEventSchema.safeParse(event)
  if (!parsed.success) {
    console.warn('Ignoring malformed scheduler stream event', parsed.error.issues)
    return null
  }
  return parsed.data
}

export function parseGroupTurnTrace(value: unknown): GroupTurnTraceResponse {
  return groupTurnTraceSchema.parse(value)
}
