/**
 * WebSocket event contract shared between backend and frontend.
 *
 * The backend defines matching Pydantic models with the same `type` literals.
 * Update both sides when adding events.
 */

export type GroupEventType =
  | 'message.created'
  | 'message.updated'
  | 'agent.status_changed'
  | 'agent.token_streamed'
  | 'thread.updated'
  | 'approval.created'
  | 'approval.resolved'
  | 'note.updated'
  | 'file.changed'
  | 'member.joined'
  | 'member.left'

export interface GroupEventBase<T extends GroupEventType, P> {
  type: T
  group_id: string
  timestamp: string
  payload: P
}

export type AgentStatus =
  | 'idle'
  | 'reading_context'
  | 'thinking'
  | 'responding'
  | 'using_tool'
  | 'waiting_user'
  | 'waiting_approval'
  | 'paused'
  | 'interrupted'
  | 'completed'
  | 'failed'
  | 'offline'

export type MessageCreatedEvent = GroupEventBase<
  'message.created',
  {
    message_id: string
    thread_id: string | null
    sender_type: 'user' | 'agent' | 'system'
    sender_id: string | null
    message_type: string
  }
>

export type AgentStatusChangedEvent = GroupEventBase<
  'agent.status_changed',
  {
    agent_id: string
    status: AgentStatus
    thread_id?: string
  }
>

export type ApprovalCreatedEvent = GroupEventBase<
  'approval.created',
  {
    approval_id: string
    agent_id: string
    approval_type: string
    title: string
  }
>

export type ApprovalResolvedEvent = GroupEventBase<
  'approval.resolved',
  {
    approval_id: string
    decision: 'approved' | 'rejected'
  }
>

export type GroupEvent =
  | MessageCreatedEvent
  | AgentStatusChangedEvent
  | ApprovalCreatedEvent
  | ApprovalResolvedEvent
