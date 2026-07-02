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
