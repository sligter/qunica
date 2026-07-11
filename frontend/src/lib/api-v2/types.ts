export type StreamEventKind =
  | 'user_message'
  | 'agent_start'
  | 'token'
  | 'reasoning'
  | 'tool_call_start'
  | 'tool_call_result'
  | 'agent_message'
  | 'agent_silent'
  | 'waiting_for_user'
  | 'context_usage'
  | 'acp_agent_run'
  | 'silence'
  | 'warning'
  | 'error'
  | 'done'

export interface StreamEvent<TPayload = unknown> {
  stream_id: string
  seq: number
  event_id: string
  kind: StreamEventKind
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

export interface GroupSchedulerConfig {
  scheduler_enabled: boolean
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
