/**
 * Hand-maintained shapes for the Rust API v2 routes the frontend currently
 * touches. Keep this file aligned with backend-rs route request/response
 * contracts until a Rust schema generation flow is introduced.
 */

import type { GroupSchedulerConfig, GroupTurnSummary } from '@/lib/api-v2/types'

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
  auto_create?: boolean
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

/** One MCP server an agent may call, optionally narrowed to specific tools. */
export interface AgentMcpServerSelection {
  server_id: string
  enabled: boolean
  /** Server-side tool names to expose; omitted or empty means every tool. */
  tools?: string[]
}

export interface AgentToolConfig {
  tools: Record<string, AgentToolSelection>
  assistant_agents?: AgentAssistantToolSelection[]
  mcp_servers?: AgentMcpServerSelection[]
}

export type McpTransport = 'stdio' | 'sse' | 'streamable-http'

export interface McpServerRead {
  id: string
  name: string
  description: string | null
  transport: McpTransport
  /** Slug used to namespace this server's tools as `mcp__<slug>__<tool>`. */
  slug: string
  command: string | null
  args: string[]
  env: Record<string, string>
  cwd: string | null
  url: string | null
  /** Header values come back masked; sending them back would store the mask. */
  headers_masked: Record<string, string>
  timeout_seconds: number
  tool_filter: string[]
  enabled: boolean
  status: string
  created_at: string
  updated_at: string
}

export interface McpServerCreate {
  name: string
  transport: McpTransport
  description?: string | null
  command?: string | null
  args?: string[]
  env?: Record<string, string>
  cwd?: string | null
  url?: string | null
  /**
   * Keep-or-set map for header values, which the API masks on the way out.
   * A string sets the header, `null` keeps whatever the server has stored, and
   * a key left out of the map deletes that header.
   */
  headers?: Record<string, string | null>
  timeout_seconds?: number
  tool_filter?: string[]
  enabled?: boolean
  /**
   * Only used by the connection-test endpoint: the saved row whose stored
   * header values a `null` entry should resolve against.
   */
  server_id?: string
}

export type McpServerUpdate = Partial<McpServerCreate>

export interface McpDiscoveredTool {
  name: string
  exposed_name: string
  description: string
}

export interface McpTestConnectionResult {
  ok: boolean
  server_label: string | null
  tools: McpDiscoveredTool[]
  error: string | null
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

export type AcpRuntimeVersionStatus =
  | 'not_installed'
  | 'current'
  | 'update_available'
  | 'local_only'

export interface AcpRuntimeVersionRead {
  id: AcpRuntimePresetRead['id']
  package_name: string
  installed: boolean
  local_version: string | null
  latest_version: string | null
  status: AcpRuntimeVersionStatus
  message: string | null
}

export interface AcpRuntimeVersionListResponse {
  presets: AcpRuntimeVersionRead[]
}

export interface AcpRuntimeInstallResponse {
  preset: AcpRuntimeVersionRead
  output: string
}

export interface AcpRuntimeCapabilitiesRead {
  models: AcpRuntimeChoice[]
  modes: AcpRuntimeChoice[]
  thinking_efforts: AcpRuntimeChoice[]
  current_model: string | null
  current_mode: string | null
  current_thinking_effort: string | null
  source: 'acp'
  warning: string | null
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
  workspace_ids?: string[]
  llm_provider_id: string | null
  skill_ids: string[]
  visibility: string
  status: string
  created_at: string
}

export interface AssistantRead {
  agent_id: string
  chat_id: string
  provider_id: string | null
  /** Model to use, or null to follow the provider's default. */
  model: string | null
  /**
   * Whether the Assistant can hold a conversation yet. False means the dock
   * shows its scripted setup checklist: an LLM agent cannot talk the user
   * through configuring the provider it needs in order to talk.
   */
  provider_configured: boolean
}

export type AppActionStatus =
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'applied'
  | 'failed'
  | 'expired'

export interface AppActionRead {
  id: string
  conversation_id: string | null
  target_kind: string
  action: string
  target_id: string | null
  summary: string
  status: AppActionStatus
  result_json: string | null
  created_at: string
  resolved_at: string | null
}

export interface AppActionList {
  items: AppActionRead[]
  has_more: boolean
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

export interface TokenUsageTotals {
  input_tokens: number
  output_tokens: number
  total_tokens: number
  calls: number
}

export interface TokenUsageSummary extends TokenUsageTotals {
  active_agents: number
}

export interface TokenUsageTimelinePoint extends TokenUsageTotals {
  date: string
}

export interface TokenUsageBreakdown extends TokenUsageTotals {
  id: string
  name: string
}

export interface TokenUsageFilterOption {
  id: string
  name: string
}

export interface TokenUsageRead {
  summary: TokenUsageSummary
  timeline: TokenUsageTimelinePoint[]
  by_group: TokenUsageBreakdown[]
  by_provider: TokenUsageBreakdown[]
  by_model: TokenUsageBreakdown[]
  by_agent: TokenUsageBreakdown[]
  filters: {
    groups: TokenUsageFilterOption[]
    providers: TokenUsageFilterOption[]
    models: TokenUsageFilterOption[]
    agents: TokenUsageFilterOption[]
  }
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
  workspace_ids?: string[]
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
  workspace_ids?: string[]
  llm_provider_id?: string | null
  skill_ids?: string[]
}

export type ProviderKind =
  | 'openai-compatible'
  | 'anthropic'
  | 'anthropic-compatible'
  | 'gemini'

export interface ProviderModelConfig {
  id: string
  context_window_tokens: number | null
  context_output_reserve_ratio: number | null
  /** Whether this model accepts a reasoning-effort setting. */
  supports_reasoning_effort?: boolean
  reasoning_passback?: boolean
}

export interface ProviderModelTestRequest {
  provider_id?: string
  kind: ProviderKind
  base_url?: string | null
  api_key?: string
  model: string
}

export interface ProviderModelTestResult {
  ok: boolean
  latency_ms: number | null
  error: string | null
}

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
  models?: ProviderModelConfig[]
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
  models?: ProviderModelConfig[]
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
  models?: ProviderModelConfig[]
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

export interface SkillUpdate {
  name?: string
  description?: string | null
  body_markdown?: string
  metadata?: Record<string, unknown> | null
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

export interface GroupRead extends GroupSchedulerConfig {
  id: string
  workspace_id: string | null
  auto_share_workspace_with_new_agents?: boolean
  name: string
  description: string | null
  announcement: string | null
  free_speech: boolean
  proactive_mode: boolean
  allow_agent_free_mention: boolean
  agent_free_mention_max_dispatches: number
  communication_mode: GroupCommunicationMode
  muted_agent_ids: string[] | null
  admin_agent_ids: string[] | null
  muted_member_ids: string[] | null
  status: string
  created_at: string
  updated_at: string
}

export type DirectChatTitleSource = 'automatic' | 'manual'

export interface DirectChatRead {
  id: string
  title: string
  title_source: DirectChatTitleSource
  agent_id: string | null
  agent_name: string | null
  agent_status: string | null
  workspace_id: string | null
  status: string
  created_at: string
  updated_at: string
}

export interface DirectChatCreate {
  agent_id: string
}

export interface DirectChatUpdate {
  title: string
}

export interface GroupCreate extends Partial<GroupSchedulerConfig> {
  name: string
  workspace_id?: string | null
  auto_share_workspace_with_new_agents?: boolean
  description?: string | null
  announcement?: string | null
  communication_mode?: GroupCommunicationMode
  initial_agents?: string[]
}

export interface GroupUpdate extends Partial<GroupSchedulerConfig> {
  name?: string
  workspace_id?: string | null
  auto_share_workspace_with_new_agents?: boolean
  description?: string | null
  announcement?: string | null
  free_speech?: boolean
  proactive_mode?: boolean
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

/**
 * Which workspace roots a group agent may address during a turn.
 *
 * - `group`: the group workspace only.
 * - `group_and_self`: the group workspace plus the agent's bound workspaces.
 * - `self`: the agent's bound workspaces only; group files and attachments are out of reach.
 */
export type GroupWorkspaceMode = 'group' | 'group_and_self' | 'self'

export interface GroupAgentRead {
  id: string
  group_id: string
  agent_id: string
  display_name: string
  role: string | null
  topology_role: GroupTopologyRole | null
  speaking_order: number | null
  response_mode: string
  workspace_mode: GroupWorkspaceMode
  /** Derived from `workspace_mode`: true unless the agent is isolated. */
  share_group_workspace: boolean
  context_usage: ContextUsage | null
  status: string
  joined_at: string
}

export interface GroupAgentAdd {
  agent_id: string
  workspace_mode?: GroupWorkspaceMode
}

export interface GroupAgentTopologyUpdate {
  topology_role?: GroupTopologyRole | null
  speaking_order?: number | null
}

export interface GroupAgentWorkspaceSharingUpdate {
  workspace_mode: GroupWorkspaceMode
}

export interface ClearGroupMessagesResponse {
  cleared_count: number
}

export interface GroupThread {
  id: string
  group_id: string
  agent_id: string | null
  created_by: string | null
  thread_type: string | null
  title: string | null
  git_branch: string | null
  goal: string | null
  status: string
  priority: number
  started_at: string | null
  completed_at: string | null
  created_at: string
  updated_at: string
}

export type WebSearchProvider = 'tavily'
export type TavilySearchDepth = 'basic' | 'advanced'
export type Appearance = 'light' | 'dark' | 'system'
export type Language = 'zh-CN' | 'en-US'

export interface SystemSettingsRead {
  id: string
  owner_id: string
  appearance: Appearance
  language: Language
  assistant_enabled: boolean
  assistant_auto_approve: boolean
  group_workspace_root: string | null
  web_search_provider: WebSearchProvider
  tavily_api_key_configured: boolean
  tavily_search_url: string
  tavily_max_results: number
  tavily_search_depth: TavilySearchDepth
  tavily_include_answer: boolean
  tavily_include_raw_content: boolean
  media_base_url: string
  media_api_key_configured: boolean
  image_generation_model: string | null
  image_generation_endpoint: string
  video_generation_model: string | null
  video_generation_endpoint: string
  video_status_endpoint: string
  video_content_endpoint: string
  created_at: string
  updated_at: string
}

export interface SystemSettingsUpdate {
  appearance?: Appearance | null
  language?: Language | null
  assistant_enabled?: boolean | null
  assistant_auto_approve?: boolean | null
  group_workspace_root?: string | null
  web_search_provider?: WebSearchProvider | null
  tavily_api_key?: string | null
  tavily_search_url?: string | null
  tavily_max_results?: number | null
  tavily_search_depth?: TavilySearchDepth | null
  tavily_include_answer?: boolean | null
  tavily_include_raw_content?: boolean | null
  media_base_url?: string | null
  media_api_key?: string | null
  image_generation_model?: string | null
  image_generation_endpoint?: string | null
  video_generation_model?: string | null
  video_generation_endpoint?: string | null
  video_status_endpoint?: string | null
  video_content_endpoint?: string | null
}

export type SenderType = 'user' | 'agent' | 'system'

export type MessageAttachmentKind = 'image' | 'file'

export interface MessageAttachment {
  id: string
  path: string
  name: string
  mime_type: string
  size: number
  kind: MessageAttachmentKind
}

export interface MessageSendInput {
  content: string
  attachments: Array<Pick<MessageAttachment, 'path'>>
  /** Model for this message only. Omitted means the agent's configured one. */
  model_override?: string
}

/** One persisted tool call, mirrored from the backend `content_json` schema. */
export interface MessageToolCall {
  tool_call_id: string | null
  tool_name: string | null
  status: string | null
  args_summary: string | null
  result_summary: string | null
}

export interface Message {
  id: string
  group_id: string
  thread_id: string | null
  sender_type: SenderType
  sender_id: string | null
  message_type: string
  content: string | null
  attachments: MessageAttachment[]
  status: string
  refs: Record<string, unknown> | null
  context_usage: ContextUsage | null
  /** Visible response bubbles, split where reasoning or tools interrupted text. */
  response_segments?: string[] | null
  /** Persisted reasoning segments (from `content_json`), in order. */
  reasoning?: string[] | null
  /** Persisted tool calls (from `content_json`), in order. */
  tool_calls?: MessageToolCall[] | null
  turn_id: string | null
  dispatch_id: string | null
  reply_to_message_id: string | null
  turn_summary: Pick<GroupTurnSummary, 'status' | 'termination_reason'> | null
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

export type ConversationScope = 'groups' | 'direct-chats'

export interface ConversationWorkspaceFileRead {
  path: string
  name: string
  is_dir: boolean
  size: number | null
  modified_at: string | null
  abs_path?: string | null
}

export interface ConversationWorkspaceRoot {
  root: string
  separator: string
}

/**
 * One browsable root inside a conversation. `agent_id === null` is the
 * conversation's own workspace; otherwise it is that member agent's folder.
 */
export interface ConversationWorkspaceRootEntry {
  agent_id: string | null
  display_name: string | null
  workspace_mode: GroupWorkspaceMode | null
  workspace_id: string
  name: string
  root: string
  /** Whether the agent's plain relative paths resolve here, or it is a mount. */
  is_primary: boolean
}

export interface ConversationWorkspaceFilePreview {
  path: string
  name: string
  is_text: boolean
  content: string | null
  truncated: boolean
  message: string | null
  size: number | null
}

export interface ConversationWorkspaceFileTextResponse {
  path: string
  name: string
  mime_type: string
  size: number
  content: string | null
  is_text: boolean
  truncated: boolean
  version: string
  message: string | null
}

export interface ConversationWorkspaceFileTextSaveRequest {
  content: string
  version: string
}

export type ConversationWorkspaceFileTextSaveResponse = ConversationWorkspaceFileTextResponse

/** Compatibility aliases retained until the group-only file panel migrates. */
export type GroupWorkspaceFileRead = ConversationWorkspaceFileRead
export type GroupWorkspaceRoot = ConversationWorkspaceRoot
export type GroupWorkspaceFilePreview = ConversationWorkspaceFilePreview

export interface GroupWorkspaceFileRename {
  new_path: string
}

export interface GroupWorkspaceGitFileStatus {
  path: string
  old_path: string | null
  status: string
  staged: boolean
  unstaged: boolean
  untracked: boolean
  conflicted: boolean
}

export interface GroupWorkspaceGitDirtyCounts {
  staged: number
  unstaged: number
  untracked: number
  conflicted: number
}

export interface GroupWorkspaceGitStatus {
  available: boolean
  status: 'ready' | 'not_repo' | 'error'
  branch: string | null
  upstream: string | null
  remote_name: string | null
  remote_url: string | null
  ahead: number | null
  behind: number | null
  stash_count: number
  clean: boolean
  dirty_counts: GroupWorkspaceGitDirtyCounts
  files: GroupWorkspaceGitFileStatus[]
  message: string | null
  state?: 'conflict' | 'detached' | 'initial' | null
}

export interface GroupWorkspaceGitPathsRequest {
  paths: string[]
}

export interface GroupWorkspaceGitCommitRequest {
  message: string
}

export interface GroupWorkspaceGitCommitMessageResponse {
  message: string
}

export type GroupWorkspaceGitDiffMode = 'worktree' | 'staged' | 'branch' | 'commit'

export interface GroupWorkspaceGitDiff {
  mode: GroupWorkspaceGitDiffMode
  base_ref: string | null
  head_ref: string | null
  path: string | null
  patch: string
  stat: string
  truncated: boolean
  binary_files: string[]
}

export interface GroupWorkspaceGitCommitSummary {
  sha: string
  short_sha: string
  subject: string
  author_name: string
  author_email: string
  author_date: string
  local_only: boolean
}

export interface GroupWorkspaceGitLog {
  commits: GroupWorkspaceGitCommitSummary[]
  has_more: boolean
}

export interface GroupWorkspaceGitCommitFile {
  path: string
  old_path: string | null
  status: string
}

export interface GroupWorkspaceGitCommitDetails {
  sha: string
  short_sha: string
  subject: string
  body: string
  author_name: string
  author_email: string
  author_date: string
  files: GroupWorkspaceGitCommitFile[]
  insertions: number
  deletions: number
  stat: string
}

export interface GroupWorkspaceGitBranch {
  name: string
  full_name: string
  kind: 'local' | 'remote'
  current: boolean
  upstream: string | null
  ahead: number
  behind: number
}

export interface GroupWorkspaceGitBranches {
  branches: GroupWorkspaceGitBranch[]
}

export interface GroupWorkspaceGitDiscardRequest {
  paths: string[]
  all: boolean
}

export interface GroupWorkspaceGitRemoteRequest {
  remote_url: string
}

export interface GroupWorkspaceGitBranchCreateRequest {
  name: string
  start_point?: string | null
}

export interface GroupWorkspaceGitBranchSwitchRequest {
  name: string
  kind?: 'local' | 'remote' | null
}

export interface GroupWorkspaceGitBranchRenameRequest {
  old: string
  new: string
}

export interface GroupWorkspaceGitBranchDeleteRequest {
  name: string
  force: boolean
}

export interface GroupWorkspaceGitInitRequest {
  branch?: string | null
}

export interface GroupWorkspaceGitIgnoreRequest {
  path: string
}

export interface GroupWorkspaceGitStashPushRequest {
  message?: string | null
}

export interface GroupWorkspaceGitCreateBranchFromCommitRequest {
  name: string
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
