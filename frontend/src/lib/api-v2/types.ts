export type LegacyStreamEventKind =
  | 'user_message'
  | 'conversation_updated'
  | 'agent_start'
  | 'token'
  | 'reasoning'
  | 'tool_call_start'
  | 'tool_call_result'
  | 'agent_message'
  | 'agent_silent'
  | 'waiting_for_user'
  | 'approval_required'
  | 'todo_update'
  | 'context_usage'
  | 'acp_agent_run'
  | 'silence'
  | 'warning'
  | 'error'
  | 'done'

export type SchedulerStreamEventKind =
  | 'turn_started'
  | 'moderator_started'
  | 'speaker_selected'
  | 'dispatch_failed'
  | 'moderator_fallback'
  | 'turn_cancelled'
  | 'turn_superseded'
  | 'turn_budget_exhausted'
  | 'turn_completed'

export type StreamEventKind = LegacyStreamEventKind | SchedulerStreamEventKind

export interface ConversationUpdatedPayload {
  conversation_id: string
  title: string
  title_source: 'automatic' | 'manual'
  updated_at: string
}

export interface StreamEvent<
  TPayload = unknown,
  TKind extends StreamEventKind = LegacyStreamEventKind,
> {
  stream_id: string
  seq: number
  event_id: string
  kind: TKind
  payload: TPayload
}

export interface ApiErrorEnvelope {
  error: {
    code: string
    message: string
    details?: unknown
  }
}

export type AgentMentionPolicy = 'display_only' | 'bounded_schedule'
export type SchedulerMode = 'bounded' | 'automatic'

export interface GroupSchedulerConfig {
  scheduler_mode: SchedulerMode
  agent_mention_policy: AgentMentionPolicy
  max_agent_steps: number | null
  max_steps_per_agent: number
  max_scheduler_hops: number
  max_moderator_calls: number
  max_consecutive_failures: number
  max_total_failures: number
  max_total_tokens: number
  turn_timeout_seconds: number
  moderator_enabled: boolean
  moderator_provider_id: string | null
  moderator_model: string | null
}

/**
 * The runtime steps a System One decision model may take over in one group.
 * Each is a separate switch; all are off until the owner turns them on, because
 * every evaluation sends conversation excerpts to the configured endpoint.
 */
export interface DecisionScenarios {
  moderator_selection: boolean
  automatic_finish: boolean
  proactive_prefilter: boolean
  shell_risk: boolean
  skill_suggestion: boolean
  note_validation: boolean
  reply_outcome: boolean
}

export type DecisionScenarioKey = keyof DecisionScenarios

/** Per-group use of the account's decision endpoint. */
export interface GroupDecisionConfig {
  decision_enabled: boolean
  decision_scenarios: DecisionScenarios
}

export type GroupTurnStatus =
  | 'queued'
  | 'pending'
  | 'running'
  | 'waiting_for_user'
  | 'completed'
  | 'silence'
  | 'budget_exhausted'
  | 'failure_budget_exhausted'
  | 'cancelled'
  | 'superseded'
  | 'failed'

export type AgentDispatchStatus =
  | 'queued'
  | 'running'
  | 'completed'
  | 'silent'
  | 'waiting_for_user'
  | 'interrupted'
  | 'cancelled'
  | 'failed'

export type SchedulerActionKind = 'speak' | 'call' | 'handoff' | 'wait' | 'silent'

export type SchedulerSelectionReason =
  | 'user_mention'
  | 'agent_call'
  | 'agent_handoff'
  | 'agent_text_mention'
  | 'deterministic_order'
  | 'moderator'
  | 'moderator_fallback'
  | 'decision_model'

export type GroupTurnTerminationReason =
  | 'waiting_for_user'
  | 'budget_exhausted'
  | 'failure_budget_exhausted'
  | 'user_cancelled'
  | 'superseded'
  | 'server_restart'
  | 'persistence_failed'
  | 'silence'
  | 'moderator_finished'

export type SchedulerDispatchFailureReason = 'persistence_failed'

export interface GroupTurnBudgetUsage {
  agent_steps: number
  moderator_calls: number
  consecutive_failures: number
  total_failures: number
  total_tokens: number
}

export interface GroupTurnBudgetLimits {
  unbounded: boolean
  max_agent_steps: number
  max_steps_per_agent: number
  max_hops: number
  max_moderator_calls: number
  max_consecutive_failures: number
  max_total_failures: number
  max_total_tokens: number
}

export interface GroupTurnTerminalBudget extends GroupTurnBudgetUsage {
  limits?: GroupTurnBudgetLimits
}

export interface GroupTurnSummary {
  id: string
  thread_id: string
  group_id: string
  trigger_message_id: string | null
  status: GroupTurnStatus
  scheduler_strategy: string
  config_snapshot: unknown
  topology_snapshot: unknown
  agent_steps: number
  moderator_calls: number
  consecutive_failures: number
  total_failures: number
  total_tokens: number
  termination_reason: GroupTurnTerminationReason | null
  created_at: string
  started_at: string | null
  completed_at: string | null
  updated_at: string
}

export interface PublicTurnArtifact {
  mode?: 'call' | 'handoff'
  target_agent_id?: string
  child_dispatch_id?: string
  outcome?: string
  failure_code?: string
  remaining_work?: string
}

export interface AgentDispatchTrace {
  id: string
  turn_id: string
  parent_dispatch_id: string | null
  source_agent_id: string | null
  target_agent_id: string
  selection_reason: SchedulerSelectionReason
  action_kind: SchedulerActionKind
  hop: number
  status: AgentDispatchStatus
  input_message_id: string | null
  output_message_id: string | null
  artifact: PublicTurnArtifact | null
  total_tokens: number
  failure_code: string | null
  created_at: string
  started_at: string | null
  completed_at: string | null
  updated_at: string
}

export interface EstimatedCost {
  amount: string
  currency: string
}

export type CostEstimationStatus = 'unavailable'

export interface GroupTurnTraceResponse {
  execution_locations?: { agent_id: string; computer_id: string; computer_name: string; generation: string }[]
  stop_confirmation_required?: boolean
  turn: GroupTurnSummary
  budget: GroupTurnBudgetUsage
  dispatches: AgentDispatchTrace[]
  estimated_cost: EstimatedCost | null
  cost_estimation_status: CostEstimationStatus
}

export interface TurnStartedPayload {
  turn_id: string
  budget: GroupTurnBudgetLimits
}

export interface ModeratorStartedPayload {
  turn_id: string
}

export interface SpeakerSelectedPayload {
  turn_id: string
  dispatch_id: string
  source_agent_id: string | null
  target_agent_id: string
  reason: SchedulerSelectionReason
  action_kind: SchedulerActionKind
  hop: number
}

export interface DispatchFailedPayload {
  turn_id: string
  dispatch_id: string
  target_agent_id: string
  action_kind: SchedulerActionKind
  reason: SchedulerDispatchFailureReason
}

export interface ModeratorFallbackPayload {
  turn_id: string
  dispatch_id: string
  target_agent_id: string
  reason: 'moderator_fallback'
}

export interface TurnTerminalPayload<
  TStatus extends GroupTurnStatus = GroupTurnStatus,
  TReason extends GroupTurnTerminationReason | null = GroupTurnTerminationReason | null,
> {
  turn_id: string
  status: TStatus
  reason: TReason
  budget: GroupTurnTerminalBudget
}

export interface SchedulerDonePayload {
  turn_id: string
}

type SchedulerEvent<K extends SchedulerStreamEventKind | 'done', TPayload> = StreamEvent<
  TPayload,
  K
>

type TurnCancelledPayload = TurnTerminalPayload<'cancelled', 'user_cancelled'>
type TurnSupersededPayload = TurnTerminalPayload<'superseded', 'superseded'>
type TurnBudgetExhaustedPayload =
  | TurnTerminalPayload<'budget_exhausted', 'budget_exhausted'>
  | TurnTerminalPayload<'failure_budget_exhausted', 'failure_budget_exhausted'>
type TurnCompletedPayload =
  | TurnTerminalPayload<'waiting_for_user', 'waiting_for_user'>
  | TurnTerminalPayload<'completed', null | 'moderator_finished'>
  | TurnTerminalPayload<'silence', 'silence'>
  | TurnTerminalPayload<'failed', 'persistence_failed'>
  | TurnTerminalPayload<'budget_exhausted', 'budget_exhausted'>
  | TurnTerminalPayload<'failure_budget_exhausted', 'failure_budget_exhausted'>

export type SchedulerStreamUpdate =
  | SchedulerEvent<'turn_started', TurnStartedPayload>
  | SchedulerEvent<'moderator_started', ModeratorStartedPayload>
  | SchedulerEvent<'speaker_selected', SpeakerSelectedPayload>
  | SchedulerEvent<'dispatch_failed', DispatchFailedPayload>
  | SchedulerEvent<'moderator_fallback', ModeratorFallbackPayload>
  | SchedulerEvent<'turn_cancelled', TurnCancelledPayload>
  | SchedulerEvent<'turn_superseded', TurnSupersededPayload>
  | SchedulerEvent<'turn_budget_exhausted', TurnBudgetExhaustedPayload>
  | SchedulerEvent<'turn_completed', TurnCompletedPayload>
  | SchedulerEvent<'done', SchedulerDonePayload>
