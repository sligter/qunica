/**
 * Hand-maintained shapes for the Rust API v2 routes the frontend currently
 * touches. Keep this file aligned with backend-rs route request/response
 * contracts until a Rust schema generation flow is introduced.
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

export type WorkspaceBackendType = 'local' | 'cloud_sandbox'

export interface WorkspaceRead {
  id: string
  name: string
  backend_type: WorkspaceBackendType
  local_path: string | null
  sandbox_ref: string | null
  config: Record<string, unknown> | null
  status: string
  created_at: string
  updated_at: string
}

export interface WorkspaceCreate {
  name: string
  backend_type?: WorkspaceBackendType
  local_path?: string | null
  sandbox_ref?: string | null
  config?: Record<string, unknown> | null
}

export interface WorkspaceUpdate {
  name?: string
  backend_type?: WorkspaceBackendType
  local_path?: string | null
  sandbox_ref?: string | null
  config?: Record<string, unknown> | null
}

export type ToolPolicy =
  | 'read'
  | 'write'
  | 'execute'
  | 'network'
  | 'media'
  | 'planning'
  | 'orchestration'

export type ToolRuntimeStatus = 'available' | 'planned' | 'sandbox_required' | 'disabled'

export interface BuiltinToolRead {
  id: string
  name: string
  description: string
  policy: ToolPolicy
  requires_workspace: boolean
  requires_sandbox: boolean
  runtime_status: ToolRuntimeStatus
}

export interface ToolCatalogResponse {
  tools: BuiltinToolRead[]
}

export interface AgentToolSelection {
  enabled: boolean
  policy?: ToolPolicy | null
}

export interface AgentAssistantToolSelection {
  agent_id: string
  enabled: boolean
}

export interface AgentToolConfig {
  tools: Record<string, AgentToolSelection>
  assistant_agents?: AgentAssistantToolSelection[]
}

export type AgentRuntimeKind = 'llm_chat' | 'acp'
export type AcpRuntimeProfile = 'custom' | 'codex' | 'claude' | 'pi' | 'opencode'
export type AcpPermissionPolicy = 'deny' | 'auto_allow'
export type AcpConfigValue = string | boolean

export interface AcpRuntimeConfig {
  profile?: AcpRuntimeProfile
  command: string
  args?: string[]
  env?: Record<string, string>
  timeout_seconds?: number | null
  permission_policy?: AcpPermissionPolicy
  model?: string | null
  mode?: string | null
  thinking_effort?: string | null
  config_options?: Record<string, AcpConfigValue> | null
}

export interface AcpRuntimeChoice {
  value: string
  label: string
  description?: string | null
}

export interface AcpRuntimePresetRead {
  id: 'codex' | 'claude' | 'pi' | 'opencode'
  name: string
  description: string
  profile: Exclude<AcpRuntimeProfile, 'custom'>
  installed: boolean
  command: string | null
  args: string[]
  env: Record<string, string>
  timeout_seconds: number
  permission_policy: AcpPermissionPolicy
  default_model: string | null
  default_mode: string | null
  default_thinking_effort: string | null
  model_options: AcpRuntimeChoice[]
  mode_options: AcpRuntimeChoice[]
  thinking_effort_options: AcpRuntimeChoice[]
  install_hint: string
  source: string | null
}

export interface AcpRuntimePresetListResponse {
  presets: AcpRuntimePresetRead[]
}

export interface AgentRead {
  id: string
  name: string
  description: string | null
  system_prompt: string
  llm_config: Record<string, unknown> | null
  tool_config: AgentToolConfig | null
  runtime_kind: AgentRuntimeKind
  acp_runtime: AcpRuntimeConfig | null
  workspace_id: string | null
  llm_provider_id: string | null
  skill_ids: string[]
  visibility: string
  status: string
  created_at: string
}

export interface ContextUsage {
  input_tokens: number | null
  output_tokens: number | null
  total_tokens: number | null
  context_window_tokens: number | null
  output_reserve_tokens: number | null
  ratio: number | null
  source: string | null
  updated_at?: string | null
}

export interface AgentCreate {
  name: string
  description?: string
  system_prompt: string
  llm_config?: Record<string, unknown> | null
  tool_config?: AgentToolConfig | null
  runtime_kind?: AgentRuntimeKind
  acp_runtime?: AcpRuntimeConfig | null
  workspace_id: string
  llm_provider_id?: string | null
  skill_ids?: string[]
}

export interface AgentUpdate {
  name?: string
  description?: string | null
  system_prompt?: string
  llm_config?: Record<string, unknown> | null
  tool_config?: AgentToolConfig | null
  runtime_kind?: AgentRuntimeKind
  acp_runtime?: AcpRuntimeConfig | null
  workspace_id?: string | null
  llm_provider_id?: string | null
  skill_ids?: string[]
}

export type ProviderKind =
  | 'openai-compatible'
  | 'anthropic'
  | 'anthropic-compatible'
  | 'gemini'

export interface LLMProviderRead {
  id: string
  name: string
  kind: ProviderKind
  base_url: string | null
  api_key_masked: string
  default_model: string
  context_window_tokens: number | null
  context_output_reserve_ratio: number | null
  description: string | null
  reasoning_passback: boolean
  status: string
  created_at: string
}

export interface LLMProviderCreate {
  name: string
  kind: ProviderKind
  base_url?: string | null
  api_key: string
  default_model: string
  context_window_tokens?: number | null
  context_output_reserve_ratio?: number | null
  description?: string | null
  reasoning_passback?: boolean
}

export interface LLMProviderUpdate {
  name?: string
  kind?: ProviderKind
  base_url?: string | null
  api_key?: string | null
  default_model?: string | null
  context_window_tokens?: number | null
  context_output_reserve_ratio?: number | null
  description?: string | null
  reasoning_passback?: boolean
}

export interface SkillFileInfo {
  path: string
  size: number
  category: string
}

export interface SkillResourceRead {
  path: string
  size: number
  category: string
  is_text: boolean
  content: string | null
}

export interface SkillResourceUpdate {
  content: string
}

export interface SkillRead {
  id: string
  name: string
  description: string | null
  body_markdown: string
  metadata: Record<string, unknown> | null
  source: string
  files: SkillFileInfo[] | null
  storage_path: string | null
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

export interface SkillGithubImport {
  url: string
  branch?: string | null
  path?: string | null
}

export type GroupCommunicationMode = 'mesh' | 'star' | 'hierarchical' | 'ring'
export type GroupTopologyRole = 'hub' | 'leader' | 'worker'

export interface GroupRead {
  id: string
  workspace_id: string | null
  name: string
  description: string | null
  announcement: string | null
  free_speech: boolean
  proactive_mode: boolean
  proactive_max_rounds: number
  proactive_reply_multiplier: number
  allow_agent_free_mention: boolean
  agent_free_mention_max_dispatches: number
  communication_mode: GroupCommunicationMode
  muted_agent_ids: string[] | null
  admin_agent_ids: string[] | null
  muted_member_ids: string[] | null
  status: string
  created_at: string
}

export interface GroupCreate {
  name: string
  workspace_id?: string | null
  description?: string | null
  announcement?: string | null
  communication_mode?: GroupCommunicationMode
  initial_agents?: string[]
}

export interface GroupUpdate {
  name?: string
  workspace_id?: string | null
  description?: string | null
  announcement?: string | null
  free_speech?: boolean
  proactive_mode?: boolean
  proactive_max_rounds?: number
  proactive_reply_multiplier?: number
  allow_agent_free_mention?: boolean
  agent_free_mention_max_dispatches?: number
  communication_mode?: GroupCommunicationMode
}

export interface GroupMemberRead {
  id: string
  group_id: string
  user_id: string
  display_name: string
  role: string
  status: string
  is_muted: boolean
  joined_at: string
}

export interface GroupMemberAdd {
  user_id: string
}

export interface GroupMuteUpdate {
  muted: boolean
}

export interface GroupAgentRead {
  id: string
  group_id: string
  agent_id: string
  display_name: string
  role: string | null
  topology_role: GroupTopologyRole | null
  speaking_order: number | null
  response_mode: string
  share_group_workspace: boolean
  context_usage: ContextUsage | null
  status: string
  joined_at: string
}

export interface GroupAgentAdd {
  agent_id: string
  share_group_workspace?: boolean
}

export interface GroupAgentTopologyUpdate {
  topology_role?: GroupTopologyRole | null
  speaking_order?: number | null
}

export interface GroupAgentWorkspaceSharingUpdate {
  share_group_workspace: boolean
}

export interface ClearGroupMessagesResponse {
  cleared_count: number
}

export type WebSearchProvider = 'tavily'
export type TavilySearchDepth = 'basic' | 'advanced'
export type Appearance = 'light' | 'dark' | 'system'

export interface SystemSettingsRead {
  id: string
  owner_id: string
  appearance: Appearance
  group_workspace_root: string | null
  web_search_provider: WebSearchProvider
  tavily_api_key_configured: boolean
  tavily_search_url: string
  tavily_max_results: number
  tavily_search_depth: TavilySearchDepth
  tavily_include_answer: boolean
  tavily_include_raw_content: boolean
  created_at: string
  updated_at: string
}

export interface SystemSettingsUpdate {
  appearance?: Appearance | null
  group_workspace_root?: string | null
  web_search_provider?: WebSearchProvider | null
  tavily_api_key?: string | null
  tavily_search_url?: string | null
  tavily_max_results?: number | null
  tavily_search_depth?: TavilySearchDepth | null
  tavily_include_answer?: boolean | null
  tavily_include_raw_content?: boolean | null
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
  context_usage: ContextUsage | null
  reply_to_message_id: string | null
  created_at: string
}

export interface SilentAgentTurn {
  agent_id: string
  display_name: string
}

export interface MessageSendResponse {
  user_message: Message
  agent_replies: Message[]
  dispatch_messages: Message[]
  warnings: string[]
  silent_turns: SilentAgentTurn[]
  all_silent: boolean
  waiting_for_user: boolean
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

export interface GroupWorkspaceFileRead {
  path: string
  name: string
  is_dir: boolean
  size: number | null
  modified_at: string | null
  abs_path?: string | null
}

export interface GroupWorkspaceRoot {
  root: string
  separator: string
}

export interface GroupWorkspaceFilePreview {
  path: string
  name: string
  is_text: boolean
  content: string | null
  truncated: boolean
  message: string | null
  size: number | null
}

export interface GroupWorkspaceFileRename {
  new_path: string
}

export interface GroupWorkspaceGitFileStatus {
  path: string
  status: string
  staged: boolean
  unstaged: boolean
}

export interface GroupWorkspaceGitStatus {
  available: boolean
  branch: string | null
  clean: boolean
  files: GroupWorkspaceGitFileStatus[]
  message: string | null
  ahead?: number | null
  behind?: number | null
  state?: 'conflict' | 'detached' | 'initial' | null
}

export interface GroupWorkspaceGitPathsRequest {
  paths: string[]
}

export interface GroupWorkspaceGitCommitRequest {
  message: string
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
