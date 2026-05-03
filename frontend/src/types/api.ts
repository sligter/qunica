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
  llm_provider_id: string | null
  skill_ids: string[]
  visibility: string
  status: string
  created_at: string
}

export interface AgentCreate {
  name: string
  description?: string
  system_prompt: string
  llm_config?: Record<string, unknown> | null
  llm_provider_id?: string | null
  skill_ids?: string[]
}

export interface AgentUpdate {
  name?: string
  description?: string | null
  system_prompt?: string
  llm_config?: Record<string, unknown> | null
  llm_provider_id?: string | null
  skill_ids?: string[]
}

export type ProviderKind = 'openai-compatible' | 'anthropic' | 'gemini'

export interface LLMProviderRead {
  id: string
  name: string
  kind: ProviderKind
  base_url: string | null
  api_key_masked: string
  default_model: string
  description: string | null
  status: string
  created_at: string
}

export interface LLMProviderCreate {
  name: string
  kind: ProviderKind
  base_url?: string | null
  api_key: string
  default_model: string
  description?: string | null
}

export interface SkillRead {
  id: string
  name: string
  description: string | null
  body_markdown: string
  source: string
  status: string
  created_at: string
}

export interface SkillCreate {
  name: string
  description?: string
  body_markdown: string
}

export interface SkillImport {
  raw: string
}

export interface GroupRead {
  id: string
  name: string
  description: string | null
  announcement: string | null
  free_speech: boolean
  allow_agent_free_mention: boolean
  muted_agent_ids: string[]
  admin_agent_ids: string[]
  status: string
  created_at: string
}

export interface GroupCreate {
  name: string
  description?: string | null
  announcement?: string | null
  initial_agents?: string[]
}

export interface GroupUpdate {
  name?: string
  description?: string | null
  announcement?: string | null
  free_speech?: boolean
  allow_agent_free_mention?: boolean
  muted_agent_ids?: string[]
  admin_agent_ids?: string[]
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

export interface ModelInfo {
  id: string
  name: string
}

export interface GroupFileRead {
  id: string
  group_id: string
  filename: string
  file_size: number
  mime_type: string | null
  created_at: string
}

export interface GroupNoteRead {
  id: string
  group_id: string
  title: string
  content: string
  created_at: string
  updated_at: string
}

export interface GroupNoteCreate {
  title: string
  content: string
}

export interface GroupNoteUpdate {
  title?: string
  content?: string
}
