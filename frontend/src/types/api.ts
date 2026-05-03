/**
 * Hand-typed shapes for the backend routes the frontend currently touches.
 *
 * When `pnpm sync-api` is wired into the dev loop, replace these with imports
 * from `@/lib/api/schema`. Until then, keep this file in sync with the
 * backend Pydantic schemas (see backend/app/schemas/).
 */

export interface UserRead {
  id: string
  email: string
  name: string
  avatar_url: string | null
  created_at: string
}

export interface Token {
  access_token: string
  token_type: 'bearer'
}

export interface AgentRead {
  id: string
  name: string
  description: string | null
  system_prompt: string
  llm_config: Record<string, unknown> | null
  visibility: string
  status: string
  created_at: string
}

export interface AgentCreate {
  name: string
  description?: string
  system_prompt: string
}

export interface GroupRead {
  id: string
  name: string
  description: string | null
  announcement: string | null
  status: string
  created_at: string
}

export interface GroupCreate {
  name: string
  description?: string | null
  announcement?: string | null
  initial_agents?: string[]
}

export interface GroupAgentRead {
  id: string
  group_id: string
  agent_id: string
  display_name: string
  role: string | null
  response_mode: string
  status: string
  joined_at: string
}

export type SenderType = 'user' | 'agent' | 'system'

export interface Message {
  id: string
  group_id: string
  thread_id: string | null
  sender_type: SenderType
  sender_id: string | null
  message_type: string
  content: string | null
  status: string
  refs: Record<string, unknown> | null
  reply_to_message_id: string | null
  created_at: string
}

export interface MessageSendResponse {
  user_message: Message
  agent_replies: Message[]
  warnings: string[]
}

export interface ApiErrorEnvelope {
  error: {
    code: string
    message: string
    details?: Record<string, unknown>
  }
}
