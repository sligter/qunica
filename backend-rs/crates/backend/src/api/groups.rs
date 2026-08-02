use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path as FsPath, PathBuf},
};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use uuid::Uuid;

use crate::api::{
    auth::current_user_id,
    error::ApiError,
    workspace_files::{self, ConversationScope},
    AppState,
};
use crate::git::{
    self as workspace_git, DiffMode, WorkspaceGitBranches, WorkspaceGitCommitDetails,
    WorkspaceGitDiff, WorkspaceGitLog, WorkspaceGitStatus,
};
use crate::llm::{
    build_provider, model_from_config, ChatDelta, ChatMessage, ChatRequest, ProviderConfig,
};
use crate::runtime::workspace_scope::WorkspaceMode;
use crate::tools::{resolve_workspace_path, ToolError};

const GROUP_COLUMNS: &str = "id, owner_id, workspace_id, name, description, announcement, \
     free_speech, proactive_mode, proactive_max_rounds, proactive_reply_multiplier, \
     allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
     scheduler_enabled, agent_mention_policy, max_agent_steps, max_steps_per_agent, \
     max_scheduler_hops, max_moderator_calls, max_consecutive_failures, \
     max_total_failures, max_total_tokens, turn_timeout_seconds, moderator_enabled, \
     moderator_provider_id, moderator_model, \
     muted_agent_ids_json, admin_agent_ids_json, muted_member_ids_json, status, \
     created_at, updated_at";

const GROUP_AGENT_COLUMNS: &str = "group_agents.group_id, group_agents.agent_id, \
     group_agents.display_name, agents.name AS agent_name, group_agents.role, \
     group_agents.topology_role, group_agents.speaking_order, group_agents.response_mode, \
     group_agents.context_scope_json, group_agents.status, group_agents.joined_at, \
     (SELECT json_extract(messages.content_json, '$.context_usage') \
      FROM messages \
      WHERE messages.group_id = group_agents.group_id \
        AND messages.sender_type = 'agent' \
        AND messages.sender_id = group_agents.agent_id \
        AND messages.status = 'visible' \
        AND json_type(messages.content_json, '$.context_usage') = 'object' \
      ORDER BY messages.created_at DESC, messages.seq DESC, messages.id DESC \
      LIMIT 1) AS context_usage_json";

const GROUP_MEMBER_COLUMNS: &str = "group_members.group_id, group_members.user_id, \
     users.name AS user_name, group_members.role, group_members.status, \
     group_members.joined_at";

const GROUP_NOTE_COLUMNS: &str = "id, group_id, title, content, created_at, updated_at";

const NOTES_DIR: &str = "Notes";
const NOTE_FILE_SUFFIX: &str = ".md";
const GROUP_FILE_COLUMNS: &str = "id, group_id, filename, file_size, mime_type, created_at";
const UPLOADS_DIR: &str = "uploads";
const MAX_WORKSPACE_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const MAX_WORKSPACE_ACTION_PATHS: usize = 1_000;
const MAX_COMMIT_DIFF_PROMPT_CHARS: usize = 20_000;
const MAX_COMMIT_SUBJECT_CHARS: usize = 72;

fn default_workspace_git_log_limit() -> usize {
    50
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitDiffQuery {
    mode: DiffMode,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitLogQuery {
    #[serde(default = "default_workspace_git_log_limit")]
    limit: usize,
    #[serde(default)]
    skip: usize,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitCommitDiffQuery {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    announcement: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    free_speech: Option<bool>,
    #[serde(default)]
    proactive_mode: Option<bool>,
    #[serde(default)]
    proactive_max_rounds: Option<i64>,
    #[serde(default)]
    proactive_reply_multiplier: Option<i64>,
    #[serde(default)]
    allow_agent_free_mention: Option<bool>,
    #[serde(default)]
    agent_free_mention_max_dispatches: Option<i64>,
    #[serde(default)]
    communication_mode: Option<String>,
    #[serde(default)]
    scheduler_enabled: Option<bool>,
    #[serde(default)]
    agent_mention_policy: Option<String>,
    #[serde(default)]
    max_agent_steps: Option<i64>,
    #[serde(default)]
    max_steps_per_agent: Option<i64>,
    #[serde(default)]
    max_scheduler_hops: Option<i64>,
    #[serde(default)]
    max_moderator_calls: Option<i64>,
    #[serde(default)]
    max_consecutive_failures: Option<i64>,
    #[serde(default)]
    max_total_failures: Option<i64>,
    #[serde(default)]
    max_total_tokens: Option<i64>,
    #[serde(default)]
    turn_timeout_seconds: Option<i64>,
    #[serde(default)]
    moderator_enabled: Option<bool>,
    #[serde(default)]
    moderator_provider_id: Option<String>,
    #[serde(default)]
    moderator_model: Option<String>,
    #[serde(default)]
    initial_agents: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    // Double `Option` distinguishes an omitted field (outer `None`) from an
    // explicit JSON `null` (inner `None`) for nullable fields.
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    announcement: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    workspace_id: Option<Option<String>>,
    #[serde(default)]
    free_speech: Option<bool>,
    #[serde(default)]
    proactive_mode: Option<bool>,
    #[serde(default)]
    proactive_max_rounds: Option<i64>,
    #[serde(default)]
    proactive_reply_multiplier: Option<i64>,
    #[serde(default)]
    allow_agent_free_mention: Option<bool>,
    #[serde(default)]
    agent_free_mention_max_dispatches: Option<i64>,
    #[serde(default)]
    communication_mode: Option<String>,
    #[serde(default)]
    scheduler_enabled: Option<bool>,
    #[serde(default)]
    agent_mention_policy: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    max_agent_steps: Option<Option<i64>>,
    #[serde(default)]
    max_steps_per_agent: Option<i64>,
    #[serde(default)]
    max_scheduler_hops: Option<i64>,
    #[serde(default)]
    max_moderator_calls: Option<i64>,
    #[serde(default)]
    max_consecutive_failures: Option<i64>,
    #[serde(default)]
    max_total_failures: Option<i64>,
    #[serde(default)]
    max_total_tokens: Option<i64>,
    #[serde(default)]
    turn_timeout_seconds: Option<i64>,
    #[serde(default)]
    moderator_enabled: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    moderator_provider_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    moderator_model: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentAddRequest {
    agent_id: String,
    #[serde(default)]
    workspace_mode: Option<String>,
    /// Legacy alias for `workspace_mode`; `true` is `group`, `false` is `self`.
    #[serde(default)]
    share_group_workspace: Option<bool>,
}

impl GroupAgentAddRequest {
    pub(crate) fn with_default_workspace(agent_id: String) -> Self {
        Self {
            agent_id,
            workspace_mode: None,
            share_group_workspace: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupMemberAddRequest {
    user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MemberCandidatesQuery {
    #[serde(default)]
    q: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupMemberMuteRequest {
    muted: bool,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentMuteRequest {
    muted: bool,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentTopologyRequest {
    #[serde(default)]
    topology_role: Option<String>,
    #[serde(default)]
    speaking_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentWorkspaceSharingRequest {
    #[serde(default)]
    workspace_mode: Option<String>,
    /// Legacy alias for `workspace_mode`; `true` is `group`, `false` is `self`.
    #[serde(default)]
    share_group_workspace: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GroupNoteCreateRequest {
    title: String,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupNoteUpdateRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceUploadQuery {
    #[serde(default)]
    unique_name: bool,
    /// Upload into this member agent's own workspace instead of the
    /// conversation's.
    #[serde(default)]
    agent_id: Option<String>,
}

impl GroupWorkspaceUploadQuery {
    /// The agent selector, treating blank as absent.
    fn agent_id(&self) -> Option<&str> {
        self.agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceFileRenameRequest {
    new_path: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupWorkspaceFileAction {
    Copy,
    Move,
    Delete,
    Clear,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceFileActionRequest {
    action: GroupWorkspaceFileAction,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    destination: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitPathsRequest {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitBranchCreateRequest {
    name: String,
    #[serde(default)]
    start_point: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitBranchSwitchRequest {
    name: String,
    #[serde(default)]
    kind: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitBranchRenameRequest {
    old: String,
    new: String,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitBranchDeleteRequest {
    name: String,
    #[serde(default)]
    force: bool,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitInitRequest {
    #[serde(default)]
    branch: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitRemoteRequest {
    remote_url: String,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitDiscardRequest {
    #[serde(default)]
    paths: Vec<String>,
    all: bool,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitIgnoreRequest {
    path: String,
}
#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitStashPushRequest {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitCommitRequest {
    message: String,
}

#[derive(Debug, Serialize)]
pub struct GroupWorkspaceGitCommitMessageResponse {
    message: String,
}

#[derive(Debug, Serialize)]
pub struct GroupResponse {
    id: String,
    workspace_id: Option<String>,
    name: String,
    description: Option<String>,
    announcement: Option<String>,
    free_speech: bool,
    proactive_mode: bool,
    proactive_max_rounds: i64,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: bool,
    agent_free_mention_max_dispatches: i64,
    communication_mode: String,
    scheduler_enabled: bool,
    agent_mention_policy: String,
    max_agent_steps: Option<i64>,
    max_steps_per_agent: i64,
    max_scheduler_hops: i64,
    max_moderator_calls: i64,
    max_consecutive_failures: i64,
    max_total_failures: i64,
    max_total_tokens: i64,
    turn_timeout_seconds: i64,
    moderator_enabled: bool,
    moderator_provider_id: Option<String>,
    moderator_model: Option<String>,
    muted_agent_ids: Option<Vec<String>>,
    admin_agent_ids: Option<Vec<String>>,
    muted_member_ids: Option<Vec<String>>,
    status: String,
    created_at: String,
    updated_at: String,
}

impl GroupResponse {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Serialize)]
pub struct GroupAgentResponse {
    id: String,
    group_id: String,
    agent_id: String,
    display_name: String,
    role: Option<String>,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
    response_mode: String,
    workspace_mode: String,
    /// Derived from `workspace_mode`, kept so older clients keep working.
    share_group_workspace: bool,
    context_usage: Option<Value>,
    status: String,
    joined_at: String,
}

#[derive(Debug, Serialize)]
pub struct GroupMemberResponse {
    id: String,
    group_id: String,
    user_id: String,
    display_name: String,
    role: String,
    status: String,
    is_muted: bool,
    joined_at: String,
}

#[derive(Debug, Serialize)]
pub struct UserReadResponse {
    id: String,
    email: String,
    name: String,
    avatar_url: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct GroupNoteResponse {
    id: String,
    group_id: String,
    title: String,
    content: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct GroupFileResponse {
    id: String,
    group_id: String,
    filename: String,
    file_size: i64,
    mime_type: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct GroupWorkspaceFileResponse {
    path: String,
    name: String,
    is_dir: bool,
    size: Option<i64>,
    modified_at: Option<String>,
    abs_path: String,
}

#[derive(Debug, Serialize)]
pub struct GroupWorkspaceRootResponse {
    root: String,
    separator: String,
}

#[derive(Debug, Serialize)]
pub struct GroupWorkspaceFilePreviewResponse {
    path: String,
    name: String,
    is_text: bool,
    content: Option<String>,
    truncated: bool,
    message: Option<String>,
    size: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupRow {
    id: String,
    owner_id: String,
    workspace_id: Option<String>,
    name: String,
    description: Option<String>,
    announcement: Option<String>,
    // SQLite stores booleans as integers; these are exposed as booleans below.
    free_speech: i64,
    proactive_mode: i64,
    proactive_max_rounds: i64,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: i64,
    agent_free_mention_max_dispatches: i64,
    communication_mode: String,
    scheduler_enabled: i64,
    agent_mention_policy: String,
    max_agent_steps: Option<i64>,
    max_steps_per_agent: i64,
    max_scheduler_hops: i64,
    max_moderator_calls: i64,
    max_consecutive_failures: i64,
    max_total_failures: i64,
    max_total_tokens: i64,
    turn_timeout_seconds: i64,
    moderator_enabled: i64,
    moderator_provider_id: Option<String>,
    moderator_model: Option<String>,
    muted_agent_ids_json: Option<String>,
    admin_agent_ids_json: Option<String>,
    muted_member_ids_json: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug)]
struct SchedulerConfigFields {
    scheduler_enabled: i64,
    agent_mention_policy: String,
    max_agent_steps: Option<i64>,
    max_steps_per_agent: i64,
    max_scheduler_hops: i64,
    max_moderator_calls: i64,
    max_consecutive_failures: i64,
    max_total_failures: i64,
    max_total_tokens: i64,
    turn_timeout_seconds: i64,
    moderator_enabled: i64,
    moderator_provider_id: Option<String>,
    moderator_model: Option<String>,
}

impl SchedulerConfigFields {
    async fn for_create(
        pool: &SqlitePool,
        owner_id: &str,
        body: &CreateRequest,
    ) -> Result<Self, ApiError> {
        Self {
            scheduler_enabled: body.scheduler_enabled.unwrap_or(false) as i64,
            agent_mention_policy: body
                .agent_mention_policy
                .as_deref()
                .unwrap_or("display_only")
                .to_string(),
            max_agent_steps: body.max_agent_steps,
            max_steps_per_agent: body.max_steps_per_agent.unwrap_or(3),
            max_scheduler_hops: body.max_scheduler_hops.unwrap_or(5),
            max_moderator_calls: body.max_moderator_calls.unwrap_or(4),
            max_consecutive_failures: body.max_consecutive_failures.unwrap_or(3),
            max_total_failures: body.max_total_failures.unwrap_or(6),
            max_total_tokens: body.max_total_tokens.unwrap_or(120_000),
            turn_timeout_seconds: body.turn_timeout_seconds.unwrap_or(300),
            moderator_enabled: body.moderator_enabled.unwrap_or(false) as i64,
            moderator_provider_id: body.moderator_provider_id.clone(),
            moderator_model: body.moderator_model.clone(),
        }
        .validate(pool, owner_id)
        .await
    }

    async fn for_update(
        pool: &SqlitePool,
        owner_id: &str,
        body: &UpdateRequest,
        existing: &GroupRow,
    ) -> Result<Self, ApiError> {
        Self {
            scheduler_enabled: body
                .scheduler_enabled
                .map(i64::from)
                .unwrap_or(existing.scheduler_enabled),
            agent_mention_policy: body
                .agent_mention_policy
                .clone()
                .unwrap_or_else(|| existing.agent_mention_policy.clone()),
            max_agent_steps: body.max_agent_steps.unwrap_or(existing.max_agent_steps),
            max_steps_per_agent: body
                .max_steps_per_agent
                .unwrap_or(existing.max_steps_per_agent),
            max_scheduler_hops: body
                .max_scheduler_hops
                .unwrap_or(existing.max_scheduler_hops),
            max_moderator_calls: body
                .max_moderator_calls
                .unwrap_or(existing.max_moderator_calls),
            max_consecutive_failures: body
                .max_consecutive_failures
                .unwrap_or(existing.max_consecutive_failures),
            max_total_failures: body
                .max_total_failures
                .unwrap_or(existing.max_total_failures),
            max_total_tokens: body.max_total_tokens.unwrap_or(existing.max_total_tokens),
            turn_timeout_seconds: body
                .turn_timeout_seconds
                .unwrap_or(existing.turn_timeout_seconds),
            moderator_enabled: body
                .moderator_enabled
                .map(i64::from)
                .unwrap_or(existing.moderator_enabled),
            moderator_provider_id: body
                .moderator_provider_id
                .clone()
                .unwrap_or_else(|| existing.moderator_provider_id.clone()),
            moderator_model: body
                .moderator_model
                .clone()
                .unwrap_or_else(|| existing.moderator_model.clone()),
        }
        .validate(pool, owner_id)
        .await
    }

    async fn validate(mut self, pool: &SqlitePool, owner_id: &str) -> Result<Self, ApiError> {
        self.agent_mention_policy = validate_agent_mention_policy(&self.agent_mention_policy)?;
        if self.max_agent_steps.is_some_and(|value| value < 1) {
            return Err(ApiError::invalid_input("max_agent_steps must be >= 1"));
        }
        validate_scheduler_minimum("max_steps_per_agent", self.max_steps_per_agent, 1)?;
        validate_scheduler_minimum("max_scheduler_hops", self.max_scheduler_hops, 0)?;
        validate_scheduler_minimum("max_moderator_calls", self.max_moderator_calls, 0)?;
        validate_scheduler_minimum("max_consecutive_failures", self.max_consecutive_failures, 1)?;
        validate_scheduler_minimum("max_total_failures", self.max_total_failures, 1)?;
        validate_scheduler_minimum("max_total_tokens", self.max_total_tokens, 1)?;
        if !(1..=3600).contains(&self.turn_timeout_seconds) {
            return Err(ApiError::invalid_input(
                "turn_timeout_seconds must be between 1 and 3600",
            ));
        }

        self.moderator_provider_id = self
            .moderator_provider_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.moderator_model = self
            .moderator_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        if self.moderator_enabled != 0 {
            let provider_id = self.moderator_provider_id.as_deref().ok_or_else(|| {
                ApiError::invalid_input(
                    "moderator_provider_id is required when moderator is enabled",
                )
            })?;
            if self.moderator_model.is_none() {
                return Err(ApiError::invalid_input(
                    "moderator_model is required when moderator is enabled",
                ));
            }
            self.moderator_provider_id =
                Some(validate_moderator_provider(pool, provider_id, owner_id).await?);
        }

        Ok(self)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ActiveGroupAgentRow {
    agent_id: String,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupAgentRow {
    group_id: String,
    agent_id: String,
    display_name: Option<String>,
    agent_name: String,
    role: Option<String>,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
    response_mode: String,
    context_scope_json: Option<String>,
    status: String,
    joined_at: String,
    context_usage_json: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupMemberRow {
    group_id: String,
    user_id: String,
    user_name: String,
    role: String,
    status: String,
    joined_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: String,
    email: String,
    name: String,
    avatar_url: Option<String>,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupNoteRow {
    id: String,
    group_id: String,
    title: String,
    content: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupFileRow {
    id: String,
    group_id: String,
    filename: String,
    file_size: i64,
    mime_type: Option<String>,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupNoteWorkspaceRow {
    owner_id: String,
    backend_type: String,
    local_path: Option<String>,
    status: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CommitMessageProviderRow {
    kind: String,
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    reasoning_passback: i64,
    models_json: Option<String>,
    model_config_json: Option<String>,
}

impl From<GroupRow> for GroupResponse {
    fn from(row: GroupRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            name: row.name,
            description: row.description,
            announcement: row.announcement,
            free_speech: row.free_speech != 0,
            proactive_mode: row.proactive_mode != 0,
            proactive_max_rounds: row.proactive_max_rounds,
            proactive_reply_multiplier: row.proactive_reply_multiplier,
            allow_agent_free_mention: row.allow_agent_free_mention != 0,
            agent_free_mention_max_dispatches: row.agent_free_mention_max_dispatches,
            communication_mode: row.communication_mode,
            scheduler_enabled: row.scheduler_enabled != 0,
            agent_mention_policy: row.agent_mention_policy,
            max_agent_steps: row.max_agent_steps,
            max_steps_per_agent: row.max_steps_per_agent,
            max_scheduler_hops: row.max_scheduler_hops,
            max_moderator_calls: row.max_moderator_calls,
            max_consecutive_failures: row.max_consecutive_failures,
            max_total_failures: row.max_total_failures,
            max_total_tokens: row.max_total_tokens,
            turn_timeout_seconds: row.turn_timeout_seconds,
            moderator_enabled: row.moderator_enabled != 0,
            moderator_provider_id: row.moderator_provider_id,
            moderator_model: row.moderator_model,
            muted_agent_ids: parse_json_list(row.muted_agent_ids_json.as_deref()),
            admin_agent_ids: parse_json_list(row.admin_agent_ids_json.as_deref()),
            muted_member_ids: parse_json_list(row.muted_member_ids_json.as_deref()),
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<GroupAgentRow> for GroupAgentResponse {
    fn from(row: GroupAgentRow) -> Self {
        let id = format!("{}:{}", row.group_id, row.agent_id);
        let workspace_mode = WorkspaceMode::from_context_scope(row.context_scope_json.as_deref());
        let context_usage = row
            .context_usage_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .filter(Value::is_object);
        Self {
            id,
            group_id: row.group_id,
            agent_id: row.agent_id,
            display_name: row.display_name.unwrap_or(row.agent_name),
            role: row.role,
            topology_role: row.topology_role,
            speaking_order: row.speaking_order,
            response_mode: row.response_mode,
            workspace_mode: workspace_mode.as_str().to_string(),
            share_group_workspace: workspace_mode.uses_group_workspace(),
            context_usage,
            status: row.status,
            joined_at: row.joined_at,
        }
    }
}

impl GroupMemberRow {
    fn into_response(self, muted_member_ids: &[String]) -> GroupMemberResponse {
        let id = format!("{}:{}", self.group_id, self.user_id);
        let is_muted = muted_member_ids
            .iter()
            .any(|value| value == self.user_id.as_str());
        GroupMemberResponse {
            id,
            group_id: self.group_id,
            user_id: self.user_id,
            display_name: self.user_name,
            role: self.role,
            status: self.status,
            is_muted,
            joined_at: self.joined_at,
        }
    }
}

impl From<UserRow> for UserReadResponse {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            name: row.name,
            avatar_url: row.avatar_url,
            created_at: row.created_at,
        }
    }
}

impl GroupNoteRow {
    fn into_response_with_content(self, content: String) -> GroupNoteResponse {
        GroupNoteResponse {
            id: self.id,
            group_id: self.group_id,
            title: self.title,
            content,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

impl From<GroupFileRow> for GroupFileResponse {
    fn from(row: GroupFileRow) -> Self {
        Self {
            id: row.id,
            group_id: row.group_id,
            filename: row.filename,
            file_size: row.file_size,
            mime_type: row.mime_type,
            created_at: row.created_at,
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<GroupResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok((
        StatusCode::CREATED,
        Json(create_inner(&state, &owner_id, body).await?),
    ))
}

/// The body of [`create`] without the axum extractors.
///
/// Approved app-actions call this, so a staged proposal runs exactly the
/// validation the UI path does. A second implementation would drift.
pub(crate) async fn create_inner(
    state: &AppState,
    owner_id: &str,
    body: CreateRequest,
) -> Result<GroupResponse, ApiError> {
    let owner_id = owner_id.to_string();

    let name = validate_name(&body.name)?;
    let description = normalize_description(body.description.as_deref());
    let announcement = normalize_description(body.announcement.as_deref());
    let free_speech = body.free_speech.unwrap_or(false);
    let proactive_mode = body.proactive_mode.unwrap_or(false);
    let proactive_max_rounds = validate_proactive_max_rounds(body.proactive_max_rounds)?;
    let multiplier = validate_multiplier(body.proactive_reply_multiplier)?;
    let allow_agent_free_mention = body.allow_agent_free_mention.unwrap_or(true);
    let agent_free_mention_max_dispatches =
        validate_agent_free_mention_max_dispatches(body.agent_free_mention_max_dispatches)?;
    let communication_mode = validate_communication_mode(body.communication_mode.as_deref())?;
    let scheduler = SchedulerConfigFields::for_create(state.db.pool(), &owner_id, &body).await?;
    let initial_agents =
        validate_initial_agents(state.db.pool(), body.initial_agents.as_deref(), &owner_id).await?;

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let workspace_id = match body.workspace_id.as_deref() {
        Some(raw) => validate_workspace(state.db.pool(), raw, &owner_id).await?,
        None => create_group_workspace(state.db.pool(), &owner_id, &id, &name, &now).await?,
    };

    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group create transaction"))?;

    sqlx::query(
        "INSERT INTO groups \
         (id, owner_id, workspace_id, name, description, announcement, free_speech, \
          proactive_mode, proactive_max_rounds, proactive_reply_multiplier, \
          allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
          scheduler_enabled, agent_mention_policy, max_agent_steps, max_steps_per_agent, \
          max_scheduler_hops, max_moderator_calls, max_consecutive_failures, \
          max_total_failures, max_total_tokens, turn_timeout_seconds, moderator_enabled, \
          moderator_provider_id, moderator_model, \
          status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
                 ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(&announcement)
    .bind(free_speech as i64)
    .bind(proactive_mode as i64)
    .bind(proactive_max_rounds)
    .bind(multiplier)
    .bind(allow_agent_free_mention as i64)
    .bind(agent_free_mention_max_dispatches)
    .bind(&communication_mode)
    .bind(scheduler.scheduler_enabled)
    .bind(&scheduler.agent_mention_policy)
    .bind(scheduler.max_agent_steps)
    .bind(scheduler.max_steps_per_agent)
    .bind(scheduler.max_scheduler_hops)
    .bind(scheduler.max_moderator_calls)
    .bind(scheduler.max_consecutive_failures)
    .bind(scheduler.max_total_failures)
    .bind(scheduler.max_total_tokens)
    .bind(scheduler.turn_timeout_seconds)
    .bind(scheduler.moderator_enabled)
    .bind(&scheduler.moderator_provider_id)
    .bind(&scheduler.moderator_model)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create group"))?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, joined_at) \
         VALUES (?, ?, 'owner', 'active', ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create owner membership"))?;

    let default_context_scope = context_scope_with_workspace_mode(None, WorkspaceMode::default())?;
    for (position, agent_id) in initial_agents.iter().enumerate() {
        let (topology_role, speaking_order) = initial_agent_topology(&communication_mode, position);
        let joined_at = now_plus_rfc3339(position as i64);
        sqlx::query(
            "INSERT INTO group_agents \
             (group_id, agent_id, topology_role, speaking_order, response_mode, \
              context_scope_json, status, joined_at, updated_at) \
             VALUES (?, ?, ?, ?, 'mentioned_only', ?, 'active', ?, ?)",
        )
        .bind(&id)
        .bind(agent_id)
        .bind(topology_role)
        .bind(speaking_order)
        .bind(&default_context_scope)
        .bind(&joined_at)
        .bind(&joined_at)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to initialize group agent"))?;
    }

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group create"))?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("group vanished after insert"))?;
    Ok(row.into())
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<GroupResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let sql = format!(
        "SELECT {GROUP_COLUMNS} FROM groups \
         WHERE owner_id = ? AND status = 'active' AND conversation_kind = 'group' \
         ORDER BY created_at DESC, id DESC"
    );
    let rows = sqlx::query_as::<_, GroupRow>(&sql)
        .bind(&owner_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(GroupResponse::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let row = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<GroupResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(
        update_inner(&state, &owner_id, &group_id, body).await?,
    ))
}

/// The body of [`update`] without the axum extractors. See [`create_inner`].
pub(crate) async fn update_inner(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    body: UpdateRequest,
) -> Result<GroupResponse, ApiError> {
    let owner_id = owner_id.to_string();
    let group_id = validate_uuid(group_id, "group id")?;

    let existing = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let scheduler =
        SchedulerConfigFields::for_update(state.db.pool(), &owner_id, &body, &existing).await?;

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    let description = match body.description {
        Some(ref value) => normalize_description(value.as_deref()),
        None => existing.description.clone(),
    };
    let announcement = match body.announcement {
        Some(ref value) => normalize_description(value.as_deref()),
        None => existing.announcement.clone(),
    };
    // `Some(Some(id))` rebinds to an owned active workspace; `Some(None)` clears
    // the binding; `None` leaves the existing binding untouched.
    let workspace_id = match body.workspace_id {
        Some(ref value) => match value.as_deref() {
            Some(raw) => Some(validate_workspace(state.db.pool(), raw, &owner_id).await?),
            None => None,
        },
        None => existing.workspace_id.clone(),
    };
    let free_speech = body
        .free_speech
        .map(|b| b as i64)
        .unwrap_or(existing.free_speech);
    let proactive_mode = body
        .proactive_mode
        .map(|b| b as i64)
        .unwrap_or(existing.proactive_mode);
    let proactive_max_rounds = match body.proactive_max_rounds {
        Some(value) => validate_proactive_max_rounds(Some(value))?,
        None => existing.proactive_max_rounds,
    };
    let multiplier = match body.proactive_reply_multiplier {
        Some(value) => validate_multiplier(Some(value))?,
        None => existing.proactive_reply_multiplier,
    };
    let allow_agent_free_mention = body
        .allow_agent_free_mention
        .map(|b| b as i64)
        .unwrap_or(existing.allow_agent_free_mention);
    let agent_free_mention_max_dispatches = match body.agent_free_mention_max_dispatches {
        Some(value) => validate_agent_free_mention_max_dispatches(Some(value))?,
        None => existing.agent_free_mention_max_dispatches,
    };
    let communication_mode = match body.communication_mode.as_deref() {
        Some(raw) => validate_communication_mode(Some(raw))?,
        None => existing.communication_mode.clone(),
    };

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group update transaction"))?;
    sqlx::query(
        "UPDATE groups SET \
         name = ?, description = ?, announcement = ?, workspace_id = ?, free_speech = ?, \
         proactive_mode = ?, proactive_max_rounds = ?, proactive_reply_multiplier = ?, \
         allow_agent_free_mention = ?, agent_free_mention_max_dispatches = ?, \
         communication_mode = ?, scheduler_enabled = ?, agent_mention_policy = ?, \
         max_agent_steps = ?, max_steps_per_agent = ?, max_scheduler_hops = ?, \
         max_moderator_calls = ?, max_consecutive_failures = ?, max_total_failures = ?, \
         max_total_tokens = ?, turn_timeout_seconds = ?, moderator_enabled = ?, \
         moderator_provider_id = ?, moderator_model = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&description)
    .bind(&announcement)
    .bind(&workspace_id)
    .bind(free_speech)
    .bind(proactive_mode)
    .bind(proactive_max_rounds)
    .bind(multiplier)
    .bind(allow_agent_free_mention)
    .bind(agent_free_mention_max_dispatches)
    .bind(&communication_mode)
    .bind(scheduler.scheduler_enabled)
    .bind(&scheduler.agent_mention_policy)
    .bind(scheduler.max_agent_steps)
    .bind(scheduler.max_steps_per_agent)
    .bind(scheduler.max_scheduler_hops)
    .bind(scheduler.max_moderator_calls)
    .bind(scheduler.max_consecutive_failures)
    .bind(scheduler.max_total_failures)
    .bind(scheduler.max_total_tokens)
    .bind(scheduler.turn_timeout_seconds)
    .bind(scheduler.moderator_enabled)
    .bind(&scheduler.moderator_provider_id)
    .bind(&scheduler.moderator_model)
    .bind(&now)
    .bind(&group_id)
    .bind(&owner_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update group"))?;

    if communication_mode != existing.communication_mode {
        normalize_group_agent_topology(&mut tx, &group_id, &communication_mode, &now).await?;
    }

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group update"))?;

    let row = fetch_row(state.db.pool(), &group_id)
        .await?
        .ok_or_else(|| ApiError::internal("group vanished after update"))?;
    Ok(row.into())
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    // Confirms existence/ownership (and that it is not already deleted) first.
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE groups SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&group_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete group"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_group_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<GroupFileResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let rows = fetch_group_file_rows(state.db.pool(), &group_id).await?;
    Ok(Json(
        rows.into_iter().map(GroupFileResponse::from).collect(),
    ))
}

pub async fn upload_group_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<GroupFileResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let upload = read_group_file_part(multipart).await?;
    let filename = validate_group_file_name(&upload.filename)?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;

    reject_active_group_file_filename(state.db.pool(), &group_id, &filename).await?;
    let path = prepare_group_upload_path(&root, &filename)?;
    write_new_group_upload_file(&path, &upload.bytes)?;

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let file_path = path.to_string_lossy().to_string();
    let file_size = upload.bytes.len() as i64;

    sqlx::query(
        "INSERT INTO group_files \
         (id, group_id, uploader_id, filename, file_path, file_size, mime_type, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'active', ?)",
    )
    .bind(&id)
    .bind(&group_id)
    .bind(&owner_id)
    .bind(&filename)
    .bind(&file_path)
    .bind(file_size)
    .bind(&upload.mime_type)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to create group file"))?;

    let row = fetch_group_file_row(state.db.pool(), &group_id, &id)
        .await?
        .ok_or_else(|| ApiError::internal("group file vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn delete_group_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, file_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let file_id = validate_uuid(&file_id, "file id")?;

    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    load_active_group_file(state.db.pool(), &group_id, &file_id).await?;

    sqlx::query(
        "UPDATE group_files SET status = 'deleted' \
         WHERE id = ? AND group_id = ? AND status = 'active'",
    )
    .bind(&file_id)
    .bind(&group_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete group file"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_group_workspace_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupWorkspaceRootResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let root = workspace_files::workspace_root(
        state.db.pool(),
        workspace_files::ConversationRoot::conversation(
            ConversationScope::Groups,
            &group_id,
            &owner_id,
        ),
    )
    .await?;
    Ok(Json(GroupWorkspaceRootResponse {
        root: root.root,
        separator: root.separator,
    }))
}

pub async fn list_group_workspace_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
) -> Result<Json<Vec<GroupWorkspaceFileResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let rows = workspace_files::list_workspace_files(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            ConversationScope::Groups,
            &group_id,
            &owner_id,
            query.agent_id(),
        ),
        &query.path,
    )
    .await?
    .into_iter()
    .map(|row| GroupWorkspaceFileResponse {
        path: row.path,
        name: row.name,
        is_dir: row.is_dir,
        size: row.size,
        modified_at: row.modified_at,
        abs_path: row.abs_path,
    })
    .collect();
    Ok(Json(rows))
}

pub async fn preview_group_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
) -> Result<Json<GroupWorkspaceFilePreviewResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let preview = workspace_files::preview_workspace_file(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            ConversationScope::Groups,
            &group_id,
            &owner_id,
            query.agent_id(),
        ),
        &query.path,
    )
    .await?;
    Ok(Json(GroupWorkspaceFilePreviewResponse {
        path: preview.path,
        name: preview.name,
        is_text: preview.is_text,
        content: preview.content,
        truncated: preview.truncated,
        message: preview.message,
        size: preview.size,
    }))
}

pub async fn upload_group_workspace_file(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: GroupWorkspaceUploadQuery,
    multipart: Multipart,
) -> Result<(StatusCode, Json<GroupWorkspaceFileResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&conversation_id, "conversation id")?;

    let root = conversation_files_root(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            scope,
            &group_id,
            &owner_id,
            query.agent_id(),
        ),
    )
    .await?;
    let upload = read_group_workspace_file_part(multipart).await?;
    let filename = unique_group_upload_filename(
        &root,
        validate_group_file_name(&upload.filename)?,
        query.unique_name,
    )?;
    let path = prepare_group_upload_path(&root, &filename)?;
    write_new_group_upload_file(&path, &upload.bytes)?;
    let path = fs::canonicalize(&path)
        .map_err(|_| ApiError::internal("failed to resolve group workspace file"))?;

    Ok((
        StatusCode::CREATED,
        Json(workspace_file_response(&path, &root)?),
    ))
}

pub async fn download_group_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
) -> Result<Response, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    workspace_files::stream_workspace_file(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            ConversationScope::Groups,
            &group_id,
            &owner_id,
            query.agent_id(),
        ),
        &query.path,
    )
    .await
}

pub async fn read_group_workspace_file_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
) -> Result<Json<workspace_files::WorkspaceFileTextResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    Ok(Json(
        workspace_files::read_workspace_file_text(
            state.db.pool(),
            workspace_files::ConversationRoot::from_query(
                ConversationScope::Groups,
                &group_id,
                &owner_id,
                query.agent_id(),
            ),
            &query.path,
        )
        .await?,
    ))
}

pub async fn save_group_workspace_file_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
    Json(body): Json<workspace_files::SaveWorkspaceFileTextRequest>,
) -> Result<Json<workspace_files::WorkspaceFileTextResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    Ok(Json(
        workspace_files::save_workspace_file_text(
            state.db.pool(),
            workspace_files::ConversationRoot::from_query(
                ConversationScope::Groups,
                &group_id,
                &owner_id,
                query.agent_id(),
            ),
            &query.path,
            &body.content,
            &body.version,
        )
        .await?,
    ))
}

pub async fn rename_group_workspace_file(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: workspace_files::WorkspaceFilePathQuery,
    body: GroupWorkspaceFileRenameRequest,
) -> Result<Json<GroupWorkspaceFileResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&conversation_id, "conversation id")?;

    let root = conversation_files_root(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            scope,
            &group_id,
            &owner_id,
            query.agent_id(),
        ),
    )
    .await?;
    let source = resolve_group_workspace_file_path(&root, &query.path)?;
    if source == root {
        return Err(ApiError::invalid_input("cannot rename the workspace root"));
    }
    if !source.exists() {
        return Err(ApiError::not_found("workspace path not found"));
    }

    let new_path = validate_workspace_file_new_path(&body.new_path)?;
    let destination = resolve_group_workspace_file_path(&root, &new_path)?;
    if path_exists_or_symlink(&destination)? {
        return Err(ApiError::conflict("destination already exists"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| ApiError::invalid_input("destination parent is invalid"))?;
    if !parent.is_dir() {
        return Err(ApiError::invalid_input("destination parent does not exist"));
    }

    fs::rename(&source, &destination)
        .map_err(|_| ApiError::internal("failed to rename workspace file"))?;
    let destination = fs::canonicalize(&destination)
        .map_err(|_| ApiError::internal("failed to resolve renamed workspace file"))?;
    Ok(Json(workspace_file_response(&destination, &root)?))
}

pub async fn delete_group_workspace_file(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: workspace_files::WorkspaceFilePathQuery,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&conversation_id, "conversation id")?;

    let root = conversation_files_root(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            scope,
            &group_id,
            &owner_id,
            query.agent_id(),
        ),
    )
    .await?;
    let target = resolve_group_workspace_entry_path(&root, &query.path)?;
    if !path_exists_or_symlink(&target)? {
        return Err(ApiError::not_found("workspace path not found"));
    }
    remove_workspace_entry(&target)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn act_on_group_workspace_files(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: workspace_files::WorkspaceFilePathQuery,
    body: GroupWorkspaceFileActionRequest,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let conversation_id = validate_uuid(&conversation_id, "conversation id")?;
    let root = conversation_files_root(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            scope,
            &conversation_id,
            &owner_id,
            query.agent_id(),
        ),
    )
    .await?;

    match body.action {
        GroupWorkspaceFileAction::Clear => clear_workspace_files(&root)?,
        GroupWorkspaceFileAction::Delete => {
            let sources = resolve_workspace_action_sources(&root, &body.paths)?;
            for source in sources {
                remove_workspace_entry(&source)?;
            }
        }
        GroupWorkspaceFileAction::Copy | GroupWorkspaceFileAction::Move => {
            let sources = resolve_workspace_action_sources(&root, &body.paths)?;
            let destination = resolve_workspace_action_destination(
                &root,
                body.destination.as_deref().unwrap_or_default(),
            )?;
            validate_action_destination(&sources, &destination)?;

            match body.action {
                GroupWorkspaceFileAction::Copy => {
                    for source in &sources {
                        validate_copy_tree(source)?;
                    }
                    let destinations = copy_destinations(&sources, &destination)?;
                    for (source, destination) in sources.iter().zip(destinations) {
                        copy_workspace_entry(source, &destination)?;
                    }
                }
                GroupWorkspaceFileAction::Move => {
                    let destinations = move_destinations(&sources, &destination)?;
                    for (source, destination) in sources.iter().zip(destinations) {
                        fs::rename(source, &destination)
                            .map_err(|_| ApiError::internal("failed to move workspace entry"))?;
                    }
                }
                GroupWorkspaceFileAction::Delete | GroupWorkspaceFileAction::Clear => {
                    unreachable!()
                }
            }
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_group_workspace_git_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    Ok(Json(workspace_git::status(&root).await))
}

async fn workspace_git_root(
    state: &AppState,
    headers: &HeaderMap,
    group_id: &str,
) -> Result<PathBuf, ApiError> {
    let owner_id = current_user_id(headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(group_id, "group id")?;
    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    group_files_workspace_root(state.db.pool(), &group, &owner_id).await
}

pub async fn get_group_workspace_git_branches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitBranches>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    Ok(Json(
        workspace_git::branches(&root)
            .await
            .map_err(workspace_git_error)?,
    ))
}
pub async fn create_group_workspace_git_branch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitBranchCreateRequest>,
) -> Result<Json<WorkspaceGitBranches>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::create_branch(
        &root,
        body.name.trim(),
        body.start_point.as_deref().map(str::trim),
    )
    .await
    .map_err(workspace_git_error)?;
    Ok(Json(
        workspace_git::branches(&root)
            .await
            .map_err(workspace_git_error)?,
    ))
}
pub async fn switch_group_workspace_git_branch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitBranchSwitchRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    if !matches!(body.kind.as_deref(), None | Some("local") | Some("remote")) {
        return Err(ApiError::invalid_input(
            "branch kind must be local or remote",
        ));
    }
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::switch_branch(&root, body.name.trim(), body.kind.as_deref())
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn rename_group_workspace_git_branch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitBranchRenameRequest>,
) -> Result<Json<WorkspaceGitBranches>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::rename_branch(&root, body.old.trim(), body.new.trim())
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(
        workspace_git::branches(&root)
            .await
            .map_err(workspace_git_error)?,
    ))
}
pub async fn delete_group_workspace_git_branch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitBranchDeleteRequest>,
) -> Result<Json<WorkspaceGitBranches>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::delete_branch(&root, body.name.trim(), body.force)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(
        workspace_git::branches(&root)
            .await
            .map_err(workspace_git_error)?,
    ))
}
pub async fn init_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitInitRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::init(&root, body.branch.as_deref().map(str::trim))
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn fetch_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::fetch(&root)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn set_group_workspace_git_remote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitRemoteRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::set_remote(&root, &body.remote_url)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn discard_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitDiscardRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    if (body.all && !body.paths.is_empty()) || (!body.all && body.paths.is_empty()) {
        return Err(ApiError::invalid_input(
            "discard requires either all: true with no paths or one or more paths with all: false",
        ));
    }
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    let paths = validate_git_paths(&root, &body.paths)?;
    workspace_git::discard(&root, &paths)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn ignore_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitIgnoreRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    let path = validate_git_paths(&root, &[body.path])?
        .into_iter()
        .next()
        .expect("single path");
    workspace_git::ignore(&root, &path)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn stash_push_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitStashPushRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::stash_push(&root, body.message.as_deref())
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}
pub async fn stash_pop_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::stash_pop(&root)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitCreateBranchRequest {
    name: String,
}

pub async fn get_group_workspace_git_diff(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceGitDiffQuery>,
) -> Result<Json<WorkspaceGitDiff>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let path = match query.path {
        Some(path) => validate_git_paths(&root, &[path])?.into_iter().next(),
        None => None,
    };
    let diff = workspace_git::diff(&root, query.mode, path.as_deref())
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(diff))
}

pub async fn get_group_workspace_git_log(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceGitLogQuery>,
) -> Result<Json<WorkspaceGitLog>, ApiError> {
    if !workspace_git::pagination_is_valid(query.limit, query.skip) {
        return Err(ApiError::invalid_input("log pagination is out of bounds"));
    }
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let log = workspace_git::log(&root, query.limit, query.skip)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(log))
}

pub async fn get_group_workspace_git_commit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, sha)): Path<(String, String)>,
) -> Result<Json<WorkspaceGitCommitDetails>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let details = workspace_git::commit_details(&root, &sha)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(details))
}

pub async fn get_group_workspace_git_commit_diff(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, sha)): Path<(String, String)>,
    Query(query): Query<GroupWorkspaceGitCommitDiffQuery>,
) -> Result<Json<WorkspaceGitDiff>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let path = match query.path {
        Some(path) => validate_git_paths(&root, &[path])?.into_iter().next(),
        None => None,
    };
    let diff = workspace_git::commit_diff(&root, &sha, path.as_deref())
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(diff))
}

pub async fn create_group_workspace_git_branch_from_commit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, sha)): Path<(String, String)>,
    Json(body): Json<GroupWorkspaceGitCreateBranchRequest>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    workspace_git::create_branch_from_commit(&root, &sha, body.name.trim())
        .await
        .map_err(workspace_git_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn stage_group_workspace_git_paths(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitPathsRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let paths = validate_git_paths(&root, &body.paths)?;
    workspace_git::stage(&root, &paths)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn unstage_group_workspace_git_paths(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitPathsRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let paths = validate_git_paths(&root, &body.paths)?;
    workspace_git::unstage(&root, &paths)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn generate_group_workspace_git_commit_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupWorkspaceGitCommitMessageResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let diff = workspace_git::staged_diff(&root)
        .await
        .map_err(workspace_git_error)?;
    if diff.trim().is_empty() {
        return Err(ApiError::invalid_input(
            "stage changes before generating a commit message",
        ));
    }

    let provider_row = load_group_commit_message_provider(state.db.pool(), &group_id, &owner_id)
        .await?
        .ok_or_else(|| {
            ApiError::invalid_input("no active LLM provider is configured for this group")
        })?;
    let model = model_from_config(&provider_row.model_config_json, &provider_row.default_model);
    let reasoning_passback = crate::llm::model_reasoning_passback(
        provider_row.models_json.as_deref(),
        &model,
        provider_row.reasoning_passback != 0,
    );
    let provider_config = ProviderConfig {
        kind: provider_row.kind,
        base_url: provider_row.base_url,
        api_key: provider_row.api_key,
        default_model: provider_row.default_model,
        reasoning_passback,
        context_window_tokens: None,
        context_output_reserve_ratio: None,
    };
    let provider = build_provider(&provider_config).map_err(|err| {
        ApiError::invalid_input(format!("commit message generation failed: {err}"))
    })?;
    let request = ChatRequest {
        model,
        messages: commit_message_prompt(&diff),
        temperature: Some(0.2),
        reasoning_passback: provider_config.reasoning_passback,
        include_empty_tools: false,
        tools: Vec::new(),
        // A commit-message generation, not a chat turn: no user override.
        reasoning_effort: None,
    };
    let mut deltas = provider.stream(request).await.map_err(|err| {
        ApiError::invalid_input(format!("commit message generation failed: {err}"))
    })?;

    let mut raw = String::new();
    while let Some(delta) = deltas.recv().await {
        match delta {
            ChatDelta::Token(text) => raw.push_str(&text),
            ChatDelta::Done => break,
            ChatDelta::Reasoning(_) | ChatDelta::ToolCall(_) | ChatDelta::Usage(_) => {}
        }
    }

    let message = clean_generated_commit_message(&raw)?;
    Ok(Json(GroupWorkspaceGitCommitMessageResponse { message }))
}

pub async fn commit_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitCommitRequest>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let message = validate_git_commit_message(&body.message)?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    workspace_git::commit(&root, message)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn pull_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    workspace_git::pull(&root)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn push_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned_workspace(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    workspace_git::push(&root)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn force_push_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::force_push(&root)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn rebase_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<WorkspaceGitStatus>, ApiError> {
    let root = workspace_git_root(&state, &headers, &group_id).await?;
    workspace_git::rebase(&root)
        .await
        .map_err(workspace_git_error)?;
    Ok(Json(workspace_git::status(&root).await))
}

pub async fn list_group_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<GroupNoteResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_notes_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let rows = fetch_group_note_rows(state.db.pool(), &group_id).await?;

    let mut notes = Vec::with_capacity(rows.len());
    for row in rows {
        let content = read_group_note_content(&root, &row.id, &row.content)?;
        notes.push(row.into_response_with_content(content));
    }
    Ok(Json(notes))
}

pub async fn create_group_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupNoteCreateRequest>,
) -> Result<(StatusCode, Json<GroupNoteResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_notes_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let title = validate_note_title(&body.title)?;
    let content = body.content.unwrap_or_default();
    let note_id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group note create transaction"))?;

    sqlx::query(
        "INSERT INTO group_notes \
         (id, group_id, author_id, title, content, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&note_id)
    .bind(&group_id)
    .bind(&owner_id)
    .bind(&title)
    .bind(&content)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create group note"))?;

    write_group_note_content(&root, &note_id, &content)?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group note create"))?;

    let row = fetch_group_note_row(state.db.pool(), &group_id, &note_id)
        .await?
        .ok_or_else(|| ApiError::internal("group note vanished after insert"))?;
    let content = read_group_note_content(&root, &row.id, &row.content)?;
    Ok((
        StatusCode::CREATED,
        Json(row.into_response_with_content(content)),
    ))
}

pub async fn update_group_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, note_id)): Path<(String, String)>,
    Json(body): Json<GroupNoteUpdateRequest>,
) -> Result<Json<GroupNoteResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let note_id = validate_uuid(&note_id, "note id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_notes_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let existing = load_active_group_note(state.db.pool(), &group_id, &note_id).await?;

    let title = match body.title.as_deref() {
        Some(raw) => validate_note_title(raw)?,
        None => existing.title,
    };
    let should_write_content = body.content.is_some();
    let content = body.content.unwrap_or(existing.content);
    let now = now_rfc3339();

    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group note update transaction"))?;

    sqlx::query(
        "UPDATE group_notes SET title = ?, content = ?, updated_at = ? \
         WHERE id = ? AND group_id = ? AND status = 'active'",
    )
    .bind(&title)
    .bind(&content)
    .bind(&now)
    .bind(&note_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update group note"))?;

    if should_write_content {
        write_group_note_content(&root, &note_id, &content)?;
    }

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group note update"))?;

    let row = fetch_group_note_row(state.db.pool(), &group_id, &note_id)
        .await?
        .ok_or_else(|| ApiError::internal("group note vanished after update"))?;
    let content = read_group_note_content(&root, &row.id, &row.content)?;
    Ok(Json(row.into_response_with_content(content)))
}

pub async fn delete_group_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, note_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let note_id = validate_uuid(&note_id, "note id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_notes_workspace_root(state.db.pool(), &group, &owner_id).await?;
    load_active_group_note(state.db.pool(), &group_id, &note_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group note delete transaction"))?;

    sqlx::query(
        "UPDATE group_notes SET status = 'deleted', updated_at = ? \
         WHERE id = ? AND group_id = ? AND status = 'active'",
    )
    .bind(&now)
    .bind(&note_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to delete group note"))?;

    delete_group_note_content(&root, &note_id)?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group note delete"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_group_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<GroupMemberResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let muted_member_ids =
        parse_json_list(group.muted_member_ids_json.as_deref()).unwrap_or_default();
    let rows = fetch_group_member_rows(state.db.pool(), &group_id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| row.into_response(&muted_member_ids))
            .collect(),
    ))
}

pub async fn search_group_member_candidates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<MemberCandidatesQuery>,
) -> Result<Json<Vec<UserReadResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let rows = search_user_rows(state.db.pool(), &query.q).await?;
    Ok(Json(rows.into_iter().map(UserReadResponse::from).collect()))
}

pub async fn add_group_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupMemberAddRequest>,
) -> Result<(StatusCode, Json<GroupMemberResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok((
        StatusCode::CREATED,
        Json(add_group_member_inner(&state, &owner_id, &group_id, &body.user_id).await?),
    ))
}

pub(crate) async fn add_group_member_inner(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    user_id: &str,
) -> Result<GroupMemberResponse, ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let user_id = validate_uuid(user_id, "user_id")?;
    let group = load_active_owned(state.db.pool(), &group_id, owner_id).await?;
    fetch_user_row(state.db.pool(), &user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("user not found"))?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group member transaction"))?;

    let existing = sqlx::query_as::<_, (String,)>(
        "SELECT status FROM group_members WHERE group_id = ? AND user_id = ?",
    )
    .bind(&group_id)
    .bind(&user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group member"))?;

    if matches!(existing.as_ref().map(|row| row.0.as_str()), Some("active")) {
        return Err(ApiError::conflict("user already in group"));
    }

    if existing.is_some() {
        sqlx::query(
            "UPDATE group_members SET role = 'member', status = 'active' \
             WHERE group_id = ? AND user_id = ?",
        )
        .bind(&group_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to reactivate group member"))?;
    } else {
        let result = sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, status, joined_at) \
             VALUES (?, ?, 'member', 'active', ?)",
        )
        .bind(&group_id)
        .bind(&user_id)
        .bind(&now)
        .execute(&mut *tx)
        .await;

        if let Err(err) = result {
            if is_unique_violation(&err) {
                return Err(ApiError::conflict("user already in group"));
            }
            return Err(ApiError::internal("failed to add group member"));
        }
    }

    touch_group(&mut tx, &group_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group member transaction"))?;

    let row = fetch_group_member_row(state.db.pool(), &group_id, &user_id)
        .await?
        .ok_or_else(|| ApiError::internal("group member vanished after add"))?;
    let muted_member_ids =
        parse_json_list(group.muted_member_ids_json.as_deref()).unwrap_or_default();
    Ok(row.into_response(&muted_member_ids))
}

pub async fn remove_group_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    remove_group_member_inner(&state, &owner_id, &group_id, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn remove_group_member_inner(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    user_id: &str,
) -> Result<(), ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let user_id = validate_uuid(user_id, "user id")?;
    load_active_owned(state.db.pool(), &group_id, owner_id).await?;
    let member = load_active_group_member(state.db.pool(), &group_id, &user_id).await?;
    if member.role == "owner" {
        return Err(ApiError::permission_denied("group owner cannot be removed"));
    }

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group member transaction"))?;

    sqlx::query(
        "UPDATE group_members SET status = 'removed' \
         WHERE group_id = ? AND user_id = ? AND status = 'active'",
    )
    .bind(&group_id)
    .bind(&user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to remove group member"))?;

    set_group_member_muted_json(&mut tx, &group_id, &user_id, false, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group member removal"))?;

    Ok(())
}

pub async fn set_group_member_muted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(String, String)>,
    Json(body): Json<GroupMemberMuteRequest>,
) -> Result<Json<GroupMemberResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let user_id = validate_uuid(&user_id, "user id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let member = load_active_group_member(state.db.pool(), &group_id, &user_id).await?;
    if member.role == "owner" {
        return Err(ApiError::permission_denied("group owner cannot be muted"));
    }

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group member mute transaction"))?;
    set_group_member_muted_json(&mut tx, &group_id, &user_id, body.muted, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group member mute update"))?;

    let group = fetch_row(state.db.pool(), &group_id)
        .await?
        .ok_or_else(|| ApiError::internal("group vanished after member mute update"))?;
    let row = fetch_group_member_row(state.db.pool(), &group_id, &user_id)
        .await?
        .ok_or_else(|| ApiError::internal("group member vanished after mute update"))?;
    let muted_member_ids =
        parse_json_list(group.muted_member_ids_json.as_deref()).unwrap_or_default();
    Ok(Json(row.into_response(&muted_member_ids)))
}

pub async fn list_group_agents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<GroupAgentResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let rows = fetch_group_agent_rows(state.db.pool(), &group_id).await?;
    Ok(Json(
        rows.into_iter().map(GroupAgentResponse::from).collect(),
    ))
}

pub async fn add_group_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupAgentAddRequest>,
) -> Result<(StatusCode, Json<GroupAgentResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok((
        StatusCode::CREATED,
        Json(add_group_agent_inner(&state, &owner_id, &group_id, body).await?),
    ))
}

pub(crate) async fn add_group_agent_inner(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    body: GroupAgentAddRequest,
) -> Result<GroupAgentResponse, ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let agent_id = validate_uuid(&body.agent_id, "agent_id")?;
    let group = load_active_owned(state.db.pool(), &group_id, owner_id).await?;
    validate_owned_active_agent(state.db.pool(), &agent_id, owner_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group agent transaction"))?;

    let existing = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, context_scope_json FROM group_agents WHERE group_id = ? AND agent_id = ?",
    )
    .bind(&group_id)
    .bind(&agent_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group agent"))?;

    if matches!(existing.as_ref().map(|row| row.0.as_str()), Some("active")) {
        return Err(ApiError::conflict("agent already in group"));
    }

    let (topology_role, speaking_order) =
        new_agent_topology(&mut tx, &group_id, &group.communication_mode).await?;
    let existing_context_scope = existing.as_ref().and_then(|row| row.1.as_deref());
    let context_scope_json = context_scope_with_workspace_mode(
        existing_context_scope,
        requested_workspace_mode(
            body.workspace_mode.as_deref(),
            body.share_group_workspace,
            WorkspaceMode::default(),
        )?,
    )?;

    if existing.is_some() {
        sqlx::query(
            "UPDATE group_agents SET \
             topology_role = ?, speaking_order = ?, response_mode = 'mentioned_only', \
             context_scope_json = ?, status = 'active', updated_at = ? \
             WHERE group_id = ? AND agent_id = ?",
        )
        .bind(&topology_role)
        .bind(speaking_order)
        .bind(&context_scope_json)
        .bind(&now)
        .bind(&group_id)
        .bind(&agent_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to reactivate group agent"))?;
    } else {
        let result = sqlx::query(
            "INSERT INTO group_agents \
             (group_id, agent_id, topology_role, speaking_order, response_mode, \
              context_scope_json, status, joined_at, updated_at) \
             VALUES (?, ?, ?, ?, 'mentioned_only', ?, 'active', ?, ?)",
        )
        .bind(&group_id)
        .bind(&agent_id)
        .bind(&topology_role)
        .bind(speaking_order)
        .bind(&context_scope_json)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await;

        if let Err(err) = result {
            if is_unique_violation(&err) {
                return Err(ApiError::conflict("agent already in group"));
            }
            return Err(ApiError::internal("failed to add group agent"));
        }
    }

    touch_group(&mut tx, &group_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group agent transaction"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after add"))?;
    Ok(row.into())
}

pub async fn remove_group_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    remove_group_agent_inner(&state, &owner_id, &group_id, &agent_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn remove_group_agent_inner(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    agent_id: &str,
) -> Result<(), ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let agent_id = validate_uuid(agent_id, "agent id")?;
    let group = load_active_owned(state.db.pool(), &group_id, owner_id).await?;
    load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group agent transaction"))?;

    sqlx::query(
        "UPDATE group_agents SET status = 'removed', updated_at = ? \
         WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(&now)
    .bind(&group_id)
    .bind(&agent_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to remove group agent"))?;

    remove_agent_from_group_lists(&mut tx, &group_id, &agent_id, &now).await?;
    // Removing the hub or the last leader would leave a topology the runtime
    // cannot schedule, so the remaining members are re-normalized.
    normalize_group_agent_topology(&mut tx, &group_id, &group.communication_mode, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group agent removal"))?;

    Ok(())
}

pub async fn set_group_agent_muted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
    Json(body): Json<GroupAgentMuteRequest>,
) -> Result<Json<GroupAgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group mute transaction"))?;
    set_group_agent_muted_json(&mut tx, &group_id, &agent_id, body.muted, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group mute update"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after mute update"))?;
    Ok(Json(row.into()))
}

pub async fn set_group_agent_workspace_sharing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
    Json(body): Json<GroupAgentWorkspaceSharingRequest>,
) -> Result<Json<GroupAgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let existing = load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let context_scope_json = context_scope_with_workspace_mode(
        existing.context_scope_json.as_deref(),
        requested_workspace_mode(
            body.workspace_mode.as_deref(),
            body.share_group_workspace,
            WorkspaceMode::from_context_scope(existing.context_scope_json.as_deref()),
        )?,
    )?;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE group_agents SET context_scope_json = ?, updated_at = ? \
         WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(&context_scope_json)
    .bind(&now)
    .bind(&group_id)
    .bind(&agent_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update group agent workspace sharing"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after workspace update"))?;
    Ok(Json(row.into()))
}

pub async fn set_group_agent_topology(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
    Json(body): Json<GroupAgentTopologyRequest>,
) -> Result<Json<GroupAgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let (topology_role, speaking_order) =
        validate_agent_topology_patch(&group.communication_mode, &body)?;
    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start topology transaction"))?;

    if topology_role.as_deref() == Some("hub") {
        sqlx::query(
            "UPDATE group_agents SET topology_role = NULL, updated_at = ? \
             WHERE group_id = ? AND agent_id <> ? AND status = 'active'",
        )
        .bind(&now)
        .bind(&group_id)
        .bind(&agent_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to clear existing topology hub"))?;
    }

    sqlx::query(
        "UPDATE group_agents SET topology_role = ?, speaking_order = ?, updated_at = ? \
         WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(&topology_role)
    .bind(speaking_order)
    .bind(&now)
    .bind(&group_id)
    .bind(&agent_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update group agent topology"))?;

    // Demoting the last leader would leave a hierarchy the runtime cannot
    // schedule, so it is rejected here rather than silently degraded.
    if group.communication_mode == "hierarchical" && topology_role.as_deref() != Some("leader") {
        let leader_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) \
             FROM group_agents \
             JOIN agents ON agents.id = group_agents.agent_id \
             WHERE group_agents.group_id = ? \
               AND group_agents.status = 'active' \
               AND agents.status = 'active' \
               AND group_agents.topology_role = 'leader'",
        )
        .bind(&group_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to verify hierarchical topology"))?;
        if leader_count == 0 {
            return Err(ApiError::invalid_input(
                "hierarchical mode needs at least one leader",
            ));
        }
    }

    touch_group(&mut tx, &group_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit topology update"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after topology update"))?;
    Ok(Json(row.into()))
}

/// Fetch an active group by id and enforce caller ownership.
///
/// Returns `404 not_found` when no row exists or it has been soft-deleted, and
/// `403 permission_denied` when an active row belongs to another user.
async fn load_active_owned(
    pool: &SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<GroupRow, ApiError> {
    let row = fetch_row(pool, group_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group not found"))?;
    if row.status == "deleted" {
        return Err(ApiError::not_found("group not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied("group belongs to another user"));
    }
    Ok(row)
}

async fn load_active_owned_workspace(
    pool: &SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<GroupRow, ApiError> {
    let sql = format!(
        "SELECT {GROUP_COLUMNS} FROM groups \
         WHERE id = ? AND conversation_kind IN ('group', 'direct')"
    );
    let row = sqlx::query_as::<_, GroupRow>(&sql)
        .bind(group_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    if row.status != "active" {
        return Err(ApiError::not_found("conversation not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "conversation belongs to another user",
        ));
    }
    Ok(row)
}

async fn fetch_row(pool: &SqlitePool, group_id: &str) -> Result<Option<GroupRow>, ApiError> {
    let sql =
        format!("SELECT {GROUP_COLUMNS} FROM groups WHERE id = ? AND conversation_kind = 'group'");
    sqlx::query_as::<_, GroupRow>(&sql)
        .bind(group_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_agent_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupAgentRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_AGENT_COLUMNS} \
         FROM group_agents \
         JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = ? \
           AND group_agents.status = 'active' \
           AND agents.status = 'active' \
         ORDER BY group_agents.joined_at ASC, group_agents.agent_id ASC"
    );
    sqlx::query_as::<_, GroupAgentRow>(&sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_agent_row(
    pool: &SqlitePool,
    group_id: &str,
    agent_id: &str,
) -> Result<Option<GroupAgentRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_AGENT_COLUMNS} \
         FROM group_agents \
         JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = ? \
           AND group_agents.agent_id = ? \
           AND group_agents.status = 'active' \
           AND agents.status = 'active'"
    );
    sqlx::query_as::<_, GroupAgentRow>(&sql)
        .bind(group_id)
        .bind(agent_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_member_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupMemberRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_MEMBER_COLUMNS} \
         FROM group_members \
         JOIN users ON users.id = group_members.user_id \
         WHERE group_members.group_id = ? \
           AND group_members.status = 'active' \
         ORDER BY group_members.joined_at ASC, group_members.user_id ASC"
    );
    sqlx::query_as::<_, GroupMemberRow>(&sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_member_row(
    pool: &SqlitePool,
    group_id: &str,
    user_id: &str,
) -> Result<Option<GroupMemberRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_MEMBER_COLUMNS} \
         FROM group_members \
         JOIN users ON users.id = group_members.user_id \
         WHERE group_members.group_id = ? \
           AND group_members.user_id = ? \
           AND group_members.status = 'active'"
    );
    sqlx::query_as::<_, GroupMemberRow>(&sql)
        .bind(group_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn load_active_group_member(
    pool: &SqlitePool,
    group_id: &str,
    user_id: &str,
) -> Result<GroupMemberRow, ApiError> {
    fetch_group_member_row(pool, group_id, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group member not found"))
}

async fn load_active_group_agent(
    pool: &SqlitePool,
    group_id: &str,
    agent_id: &str,
) -> Result<GroupAgentRow, ApiError> {
    fetch_group_agent_row(pool, group_id, agent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group agent not found"))
}

async fn fetch_group_note_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupNoteRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_NOTE_COLUMNS} FROM group_notes \
         WHERE group_id = ? AND status = 'active' \
         ORDER BY updated_at DESC, id DESC"
    );
    sqlx::query_as::<_, GroupNoteRow>(&sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_note_row(
    pool: &SqlitePool,
    group_id: &str,
    note_id: &str,
) -> Result<Option<GroupNoteRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_NOTE_COLUMNS} FROM group_notes \
         WHERE group_id = ? AND id = ? AND status = 'active'"
    );
    sqlx::query_as::<_, GroupNoteRow>(&sql)
        .bind(group_id)
        .bind(note_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn load_active_group_note(
    pool: &SqlitePool,
    group_id: &str,
    note_id: &str,
) -> Result<GroupNoteRow, ApiError> {
    fetch_group_note_row(pool, group_id, note_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group note not found"))
}

async fn fetch_group_file_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupFileRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_FILE_COLUMNS} FROM group_files \
         WHERE group_id = ? AND status = 'active' \
         ORDER BY created_at DESC, id DESC"
    );
    sqlx::query_as::<_, GroupFileRow>(&sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_file_row(
    pool: &SqlitePool,
    group_id: &str,
    file_id: &str,
) -> Result<Option<GroupFileRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_FILE_COLUMNS} FROM group_files \
         WHERE group_id = ? AND id = ? AND status = 'active'"
    );
    sqlx::query_as::<_, GroupFileRow>(&sql)
        .bind(group_id)
        .bind(file_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn load_active_group_file(
    pool: &SqlitePool,
    group_id: &str,
    file_id: &str,
) -> Result<GroupFileRow, ApiError> {
    fetch_group_file_row(pool, group_id, file_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group file not found"))
}

async fn reject_active_group_file_filename(
    pool: &SqlitePool,
    group_id: &str,
    filename: &str,
) -> Result<(), ApiError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_files \
         WHERE group_id = ? AND filename = ? AND status = 'active'",
    )
    .bind(group_id)
    .bind(filename)
    .fetch_one(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    if exists > 0 {
        return Err(ApiError::conflict(
            "a file with this name already exists in uploads",
        ));
    }
    Ok(())
}

// Thin Axum adapters binding the group URL namespace to the shared services.
pub async fn upload_workspace_file_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceUploadQuery>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<GroupWorkspaceFileResponse>), ApiError> {
    upload_group_workspace_file(
        state,
        headers,
        ConversationScope::Groups,
        group_id,
        query,
        multipart,
    )
    .await
}

pub async fn rename_workspace_file_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
    Json(body): Json<GroupWorkspaceFileRenameRequest>,
) -> Result<Json<GroupWorkspaceFileResponse>, ApiError> {
    rename_group_workspace_file(
        state,
        headers,
        ConversationScope::Groups,
        group_id,
        query,
        body,
    )
    .await
}

pub async fn delete_workspace_file_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
) -> Result<StatusCode, ApiError> {
    delete_group_workspace_file(state, headers, ConversationScope::Groups, group_id, query).await
}

pub async fn workspace_file_actions_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<workspace_files::WorkspaceFilePathQuery>,
    Json(body): Json<GroupWorkspaceFileActionRequest>,
) -> Result<StatusCode, ApiError> {
    act_on_group_workspace_files(
        state,
        headers,
        ConversationScope::Groups,
        group_id,
        query,
        body,
    )
    .await
}

pub async fn list_workspace_roots_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<workspace_files::ConversationRootEntry>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    Ok(Json(
        workspace_files::list_conversation_roots(
            state.db.pool(),
            ConversationScope::Groups,
            &group_id,
            &owner_id,
        )
        .await?,
    ))
}

/// Canonical root for a conversation file mutation, honouring the addressed
/// root. Delegates to the shared service so uploads, renames and deletes get
/// the same authorization and kind checks the read endpoints already apply.
async fn conversation_files_root(
    pool: &SqlitePool,
    target: workspace_files::ConversationRoot<'_>,
) -> Result<PathBuf, ApiError> {
    Ok(workspace_files::load_owned_local_workspace(pool, target)
        .await?
        .root)
}

#[allow(dead_code)]
async fn group_files_workspace_root(
    pool: &SqlitePool,
    group: &GroupRow,
    owner_id: &str,
) -> Result<PathBuf, ApiError> {
    let workspace_id = group
        .workspace_id
        .as_deref()
        .ok_or_else(|| ApiError::invalid_input("group has no bound workspace"))?;
    let row = sqlx::query_as::<_, GroupNoteWorkspaceRow>(
        "SELECT owner_id, backend_type, local_path, status FROM workspaces WHERE id = ?",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::invalid_input("group workspace is not active"))?;

    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "group workspace belongs to another user",
        ));
    }
    if row.status != "active" {
        return Err(ApiError::invalid_input("group workspace is not active"));
    }
    if row.backend_type != "local" {
        return Err(ApiError::invalid_input(
            "group file uploads require a local workspace",
        ));
    }
    let local_path = row
        .local_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| ApiError::invalid_input("local workspace has no local_path"))?;
    let root = fs::canonicalize(local_path).map_err(|_| {
        ApiError::invalid_input("group workspace path must be an existing directory")
    })?;
    if !root.is_dir() {
        return Err(ApiError::invalid_input(
            "group workspace path must be an existing directory",
        ));
    }
    Ok(root)
}

fn validate_git_paths(root: &FsPath, raw_paths: &[String]) -> Result<Vec<String>, ApiError> {
    let mut paths = Vec::with_capacity(raw_paths.len());
    for raw in raw_paths {
        let path = raw.trim();
        if path.is_empty() {
            return Err(ApiError::invalid_input("git path must be non-empty"));
        }
        let Some(relative) = workspace_file_relative_path(path)? else {
            return Err(ApiError::invalid_input("git path must be non-empty"));
        };
        resolve_workspace_path(root, &relative).map_err(workspace_path_error)?;
        paths.push(relative);
    }
    Ok(paths)
}

fn workspace_git_error(err: workspace_git::GitOperationError) -> ApiError {
    if err.code() == Some("missing_remote") {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing_remote",
            "git remote is not configured; set a remote URL before fetch, pull, or push",
        )
    } else {
        ApiError::invalid_input(err.to_string())
    }
}

async fn load_group_commit_message_provider(
    pool: &SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<Option<CommitMessageProviderRow>, ApiError> {
    sqlx::query_as(
        "SELECT p.kind, p.base_url, p.api_key, p.default_model, p.reasoning_passback, \
                p.models_json, a.model_config_json \
         FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         JOIN llm_providers p ON p.id = a.provider_id \
         WHERE ga.group_id = ? \
           AND ga.status = 'active' \
           AND a.status = 'active' \
           AND a.owner_id = ? \
           AND a.runtime_kind = 'llm_chat' \
           AND a.provider_id IS NOT NULL \
           AND p.owner_id = ? \
           AND p.status = 'active' \
         ORDER BY COALESCE(NULLIF(ga.speaking_order, 0), 9223372036854775807) ASC, \
                  ga.joined_at ASC, a.id ASC \
         LIMIT 1",
    )
    .bind(group_id)
    .bind(owner_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

fn commit_message_prompt(diff: &str) -> Vec<ChatMessage> {
    let mut prompt_diff: String = diff.chars().take(MAX_COMMIT_DIFF_PROMPT_CHARS).collect();
    if diff.chars().count() > MAX_COMMIT_DIFF_PROMPT_CHARS {
        prompt_diff.push_str("\n[diff truncated]");
    }

    vec![
        ChatMessage::text(
            "system",
            "Write one concise Git commit subject for the staged diff. Return only the subject line. No markdown, quotes, bullets, or prefixes. Use imperative mood when natural. Keep it under 72 characters.",
        ),
        ChatMessage::text("user", prompt_diff),
    ]
}

fn clean_generated_commit_message(raw: &str) -> Result<String, ApiError> {
    let without_fences = raw
        .lines()
        .filter(|line| !line.trim().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n");

    for line in without_fences.lines() {
        let mut message = line.trim();
        if message.is_empty() {
            continue;
        }
        message = message
            .strip_prefix("- ")
            .or_else(|| message.strip_prefix("* "))
            .unwrap_or(message)
            .trim();

        let mut message = strip_wrapping_quotes(message).trim().to_string();
        if message.chars().count() > MAX_COMMIT_SUBJECT_CHARS {
            message = message.chars().take(MAX_COMMIT_SUBJECT_CHARS).collect();
        }
        if !message.is_empty() {
            return Ok(message);
        }
    }

    Err(ApiError::invalid_input(
        "provider returned an empty commit message",
    ))
}

fn strip_wrapping_quotes(message: &str) -> &str {
    let trimmed = message.trim();
    if trimmed.len() < 2 {
        return trimmed;
    }
    let pairs = [('"', '"'), ('\'', '\''), ('`', '`')];
    for (open, close) in pairs {
        if trimmed.starts_with(open) && trimmed.ends_with(close) {
            return &trimmed[open.len_utf8()..trimmed.len() - close.len_utf8()];
        }
    }
    trimmed
}

fn validate_git_commit_message(raw: &str) -> Result<String, ApiError> {
    let message = raw.trim().to_string();
    if message.is_empty() {
        return Err(ApiError::invalid_input("commit message is required"));
    }
    if message.chars().count() > 10_000 {
        return Err(ApiError::invalid_input(
            "commit message must be at most 10000 characters",
        ));
    }
    Ok(message)
}

async fn group_notes_workspace_root(
    pool: &SqlitePool,
    group: &GroupRow,
    owner_id: &str,
) -> Result<PathBuf, ApiError> {
    let workspace_id = group
        .workspace_id
        .as_deref()
        .ok_or_else(|| ApiError::invalid_input("group has no bound workspace"))?;
    let row = sqlx::query_as::<_, GroupNoteWorkspaceRow>(
        "SELECT owner_id, backend_type, local_path, status FROM workspaces WHERE id = ?",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::invalid_input("group workspace is not active"))?;

    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "group workspace belongs to another user",
        ));
    }
    if row.status != "active" {
        return Err(ApiError::invalid_input("group workspace is not active"));
    }
    if row.backend_type != "local" {
        return Err(ApiError::invalid_input(
            "group notes require a local workspace",
        ));
    }
    let local_path = row
        .local_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| ApiError::invalid_input("local workspace has no local_path"))?;
    let root = fs::canonicalize(local_path).map_err(|_| {
        ApiError::invalid_input("group workspace path must be an existing directory")
    })?;
    if !root.is_dir() {
        return Err(ApiError::invalid_input(
            "group workspace path must be an existing directory",
        ));
    }
    Ok(root)
}

async fn fetch_user_row(pool: &SqlitePool, user_id: &str) -> Result<Option<UserRow>, ApiError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, avatar_url, created_at FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

async fn search_user_rows(pool: &SqlitePool, query: &str) -> Result<Vec<UserRow>, ApiError> {
    let trimmed = query.trim().to_lowercase();
    if trimmed.is_empty() {
        return sqlx::query_as::<_, UserRow>(
            "SELECT id, email, name, avatar_url, created_at \
             FROM users \
             ORDER BY created_at DESC, id DESC \
             LIMIT 20",
        )
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"));
    }

    let pattern = format!("%{trimmed}%");
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, avatar_url, created_at \
         FROM users \
         WHERE LOWER(name) LIKE ? OR LOWER(email) LIKE ? \
         ORDER BY created_at DESC, id DESC \
         LIMIT 20",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

async fn validate_owned_active_agent(
    pool: &SqlitePool,
    agent_id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT owner_id, status FROM agents WHERE id = ?")
            .bind(agent_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::not_found("agent not found")),
        Some((_, status)) if status != "active" => Err(ApiError::not_found("agent not found")),
        Some((owner, _)) if owner != owner_id => {
            Err(ApiError::permission_denied("agent belongs to another user"))
        }
        Some(_) => Ok(()),
    }
}

async fn new_agent_topology(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    mode: &str,
) -> Result<(Option<String>, Option<i64>), ApiError> {
    match mode {
        "mesh" => Ok((None, None)),
        "star" => {
            let active_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) \
                 FROM group_agents \
                 JOIN agents ON agents.id = group_agents.agent_id \
                 WHERE group_agents.group_id = ? \
                   AND group_agents.status = 'active' \
                   AND agents.status = 'active'",
            )
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load star topology state"))?;
            let role = if active_count == 0 {
                Some("hub".to_string())
            } else {
                None
            };
            Ok((role, None))
        }
        "hierarchical" => {
            // The runtime rejects a hierarchy with no leader, so the first agent
            // in the group takes that role.
            let leader_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) \
                 FROM group_agents \
                 JOIN agents ON agents.id = group_agents.agent_id \
                 WHERE group_agents.group_id = ? \
                   AND group_agents.status = 'active' \
                   AND agents.status = 'active' \
                   AND group_agents.topology_role = 'leader'",
            )
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load hierarchical topology state"))?;
            let role = if leader_count == 0 {
                "leader"
            } else {
                "worker"
            };
            Ok((Some(role.to_string()), None))
        }
        "ring" => {
            let max_order: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(group_agents.speaking_order) \
                 FROM group_agents \
                 JOIN agents ON agents.id = group_agents.agent_id \
                 WHERE group_agents.group_id = ? \
                   AND group_agents.status = 'active' \
                   AND agents.status = 'active' \
                   AND group_agents.speaking_order > 0",
            )
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load ring topology state"))?;
            Ok((None, Some(max_order.unwrap_or_default() + 1)))
        }
        _ => Err(ApiError::internal("unsupported communication mode")),
    }
}

fn validate_agent_topology_patch(
    mode: &str,
    body: &GroupAgentTopologyRequest,
) -> Result<(Option<String>, Option<i64>), ApiError> {
    let role = body
        .topology_role
        .as_deref()
        .map(str::trim)
        .map(str::to_string);

    match mode {
        "mesh" => {
            if role.is_some() || body.speaking_order.is_some() {
                return Err(ApiError::invalid_input(
                    "mesh mode does not use agent topology settings",
                ));
            }
            Ok((None, None))
        }
        "star" => {
            if body.speaking_order.is_some() || !matches!(role.as_deref(), None | Some("hub")) {
                return Err(ApiError::invalid_input(
                    "star mode only accepts hub topology role",
                ));
            }
            Ok((role, None))
        }
        "hierarchical" => {
            if body.speaking_order.is_some()
                || !matches!(role.as_deref(), None | Some("leader") | Some("worker"))
            {
                return Err(ApiError::invalid_input(
                    "hierarchical mode accepts leader or worker topology role",
                ));
            }
            Ok((role, None))
        }
        "ring" => {
            if role.is_some() {
                return Err(ApiError::invalid_input(
                    "ring mode only accepts speaking order",
                ));
            }
            if matches!(body.speaking_order, Some(order) if order < 1) {
                return Err(ApiError::invalid_input(
                    "ring speaking_order must be null or >= 1",
                ));
            }
            Ok((None, body.speaking_order))
        }
        _ => Err(ApiError::internal("unsupported communication mode")),
    }
}

async fn touch_group(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE groups SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(group_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group timestamp"))?;
    Ok(())
}

async fn set_group_agent_muted_json(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    agent_id: &str,
    muted: bool,
    now: &str,
) -> Result<(), ApiError> {
    let (raw_muted,): (Option<String>,) =
        sqlx::query_as("SELECT muted_agent_ids_json FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load group mute list"))?;
    let muted_agent_ids_json = if muted {
        add_to_json_list(raw_muted.as_deref(), agent_id)?
    } else {
        remove_from_json_list(raw_muted.as_deref(), agent_id)?
    };
    sqlx::query("UPDATE groups SET muted_agent_ids_json = ?, updated_at = ? WHERE id = ?")
        .bind(&muted_agent_ids_json)
        .bind(now)
        .bind(group_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group mute list"))?;
    Ok(())
}

async fn set_group_member_muted_json(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    user_id: &str,
    muted: bool,
    now: &str,
) -> Result<(), ApiError> {
    let (raw_muted,): (Option<String>,) =
        sqlx::query_as("SELECT muted_member_ids_json FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load group member mute list"))?;
    let muted_member_ids_json = if muted {
        add_to_json_list(raw_muted.as_deref(), user_id)?
    } else {
        remove_from_json_list(raw_muted.as_deref(), user_id)?
    };
    sqlx::query("UPDATE groups SET muted_member_ids_json = ?, updated_at = ? WHERE id = ?")
        .bind(&muted_member_ids_json)
        .bind(now)
        .bind(group_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group member mute list"))?;
    Ok(())
}

async fn remove_agent_from_group_lists(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    agent_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    let (raw_muted, raw_admin): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT muted_agent_ids_json, admin_agent_ids_json FROM groups WHERE id = ?",
    )
    .bind(group_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group agent lists"))?;
    let muted_agent_ids_json = remove_from_json_list(raw_muted.as_deref(), agent_id)?;
    let admin_agent_ids_json = remove_from_json_list(raw_admin.as_deref(), agent_id)?;
    sqlx::query(
        "UPDATE groups SET muted_agent_ids_json = ?, admin_agent_ids_json = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&muted_agent_ids_json)
    .bind(&admin_agent_ids_json)
    .bind(now)
    .bind(group_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApiError::internal("failed to clear group agent lists"))?;
    Ok(())
}

fn add_to_json_list(raw: Option<&str>, item: &str) -> Result<String, ApiError> {
    let mut values = parse_json_list(raw).unwrap_or_default();
    if !values.iter().any(|value| value == item) {
        values.push(item.to_string());
    }
    json_list_to_db(values)
}

fn remove_from_json_list(raw: Option<&str>, item: &str) -> Result<String, ApiError> {
    let mut values = parse_json_list(raw).unwrap_or_default();
    values.retain(|value| value != item);
    json_list_to_db(values)
}

fn json_list_to_db(values: Vec<String>) -> Result<String, ApiError> {
    serde_json::to_string(&values)
        .map_err(|_| ApiError::internal("failed to serialize group id list"))
}

/// Resolve the workspace mode a request asks for.
///
/// An explicit `workspace_mode` wins; the legacy `share_group_workspace`
/// boolean is honoured for older clients; absent means `default`.
fn requested_workspace_mode(
    workspace_mode: Option<&str>,
    share_group_workspace: Option<bool>,
    default: WorkspaceMode,
) -> Result<WorkspaceMode, ApiError> {
    if let Some(raw) = workspace_mode {
        return WorkspaceMode::parse(raw).ok_or_else(|| {
            ApiError::invalid_input("workspace_mode must be group, group_and_self, or self")
        });
    }
    Ok(match share_group_workspace {
        Some(true) => WorkspaceMode::Group,
        Some(false) => WorkspaceMode::SelfOnly,
        None => default,
    })
}

fn context_scope_with_workspace_mode(
    raw: Option<&str>,
    mode: WorkspaceMode,
) -> Result<Option<String>, ApiError> {
    mode.to_context_scope(raw)
        .map_err(|_| ApiError::internal("failed to serialize context scope"))
}

async fn normalize_group_agent_topology(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    mode: &str,
    now: &str,
) -> Result<(), ApiError> {
    let rows = sqlx::query_as::<_, ActiveGroupAgentRow>(
        "SELECT group_agents.agent_id, group_agents.topology_role, group_agents.speaking_order \
         FROM group_agents \
         JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = ? \
           AND group_agents.status = 'active' \
           AND agents.status = 'active' \
         ORDER BY group_agents.joined_at ASC, group_agents.agent_id ASC",
    )
    .bind(group_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group agents for topology update"))?;

    let updates = match mode {
        "mesh" => rows
            .iter()
            .map(|row| (row.agent_id.clone(), None, None))
            .collect(),
        "star" => star_topology_updates(&rows),
        "hierarchical" => hierarchical_topology_updates(&rows),
        "ring" => ring_topology_updates(&rows),
        _ => return Err(ApiError::internal("unsupported communication mode")),
    };

    for (agent_id, topology_role, speaking_order) in updates {
        sqlx::query(
            "UPDATE group_agents \
             SET topology_role = ?, speaking_order = ?, updated_at = ? \
             WHERE group_id = ? AND agent_id = ? AND status = 'active'",
        )
        .bind(topology_role)
        .bind(speaking_order)
        .bind(now)
        .bind(group_id)
        .bind(agent_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group agent topology"))?;
    }

    Ok(())
}

fn star_topology_updates(
    rows: &[ActiveGroupAgentRow],
) -> Vec<(String, Option<String>, Option<i64>)> {
    let hub_agent_id = rows
        .iter()
        .find(|row| row.topology_role.as_deref() == Some("hub"))
        .or_else(|| rows.first())
        .map(|row| row.agent_id.as_str());

    rows.iter()
        .map(|row| {
            let role = if hub_agent_id == Some(row.agent_id.as_str()) {
                Some("hub".to_string())
            } else {
                None
            };
            (row.agent_id.clone(), role, None)
        })
        .collect()
}

fn hierarchical_topology_updates(
    rows: &[ActiveGroupAgentRow],
) -> Vec<(String, Option<String>, Option<i64>)> {
    // A hierarchy with no leader is rejected by the runtime, so when nobody
    // carries the role the earliest-joined agent is promoted.
    let implicit_leader_agent_id = rows
        .iter()
        .all(|row| row.topology_role.as_deref() != Some("leader"))
        .then(|| rows.first().map(|row| row.agent_id.as_str()))
        .flatten();

    rows.iter()
        .map(|row| {
            let role = match row.topology_role.as_deref() {
                Some("leader") => "leader",
                _ if implicit_leader_agent_id == Some(row.agent_id.as_str()) => "leader",
                _ => "worker",
            };
            (row.agent_id.clone(), Some(role.to_string()), None)
        })
        .collect()
}

fn ring_topology_updates(
    rows: &[ActiveGroupAgentRow],
) -> Vec<(String, Option<String>, Option<i64>)> {
    let mut order_counts = BTreeMap::new();
    for row in rows {
        if let Some(order) = row.speaking_order.filter(|order| *order > 0) {
            *order_counts.entry(order).or_insert(0usize) += 1;
        }
    }

    let mut used_orders = BTreeSet::new();
    let mut updates = Vec::with_capacity(rows.len());
    for row in rows {
        let order = row.speaking_order.filter(|order| {
            *order > 0 && order_counts.get(order).copied().unwrap_or_default() == 1
        });
        if let Some(order) = order {
            used_orders.insert(order);
        }
        updates.push((row.agent_id.clone(), None, order));
    }

    let mut next_order = used_orders.iter().next_back().copied().unwrap_or_default() + 1;
    for (_, _, speaking_order) in &mut updates {
        if speaking_order.is_some() {
            continue;
        }
        while used_orders.contains(&next_order) {
            next_order += 1;
        }
        *speaking_order = Some(next_order);
        used_orders.insert(next_order);
        next_order += 1;
    }

    updates
}

/// Resolve a workspace reference to its canonical id, requiring it to be an
/// active workspace owned by the caller.
async fn validate_workspace(
    pool: &SqlitePool,
    raw_id: &str,
    owner_id: &str,
) -> Result<String, ApiError> {
    let id = validate_uuid(raw_id, "workspace_id")?;
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT owner_id, status FROM workspaces WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::invalid_input(
            "workspace_id does not reference a workspace",
        )),
        Some((owner, _)) if owner != owner_id => Err(ApiError::permission_denied(
            "workspace belongs to another user",
        )),
        Some((_, status)) if status != "active" => {
            Err(ApiError::invalid_input("workspace is not active"))
        }
        Some(_) => Ok(id),
    }
}

/// Directory name for an auto-created group workspace.
///
/// A bare UUID is unrecognisable in a file manager or a workspace picker, so
/// lead with a slug of the group name and keep a short id for uniqueness. Groups
/// whose names slugify to nothing (emoji, punctuation, some scripts) fall back
/// to the id alone rather than producing a bare separator.
fn group_workspace_dir_name(group_name: &str, group_id: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;
    for ch in group_name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            pending_separator = false;
            slug.extend(ch.to_lowercase());
        } else {
            pending_separator = true;
        }
        if slug.len() >= 40 {
            break;
        }
    }
    let short_id: String = group_id.chars().filter(|ch| *ch != '-').take(8).collect();
    if slug.is_empty() {
        short_id
    } else {
        format!("{slug}-{short_id}")
    }
}

async fn create_group_workspace(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
    group_name: &str,
    now: &str,
) -> Result<String, ApiError> {
    let root = require_group_workspace_root(pool, owner_id).await?;
    let storage_dir = PathBuf::from(root).join(group_workspace_dir_name(group_name, group_id));
    std::fs::create_dir_all(&storage_dir)
        .map_err(|_| ApiError::internal("failed to create group workspace directory"))?;
    let local_path = std::fs::canonicalize(&storage_dir)
        .map_err(|_| ApiError::internal("failed to resolve group workspace directory"))?
        .to_string_lossy()
        .into_owned();

    let workspace_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO workspaces \
         (id, owner_id, name, backend_type, local_path, config_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', ?, NULL, 'active', ?, ?)",
    )
    .bind(&workspace_id)
    .bind(owner_id)
    .bind(group_name)
    .bind(&local_path)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("failed to create group workspace"))?;

    Ok(workspace_id)
}

async fn require_group_workspace_root(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<String, ApiError> {
    let row = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT group_workspace_root FROM system_settings WHERE owner_id = ?",
    )
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    row.and_then(|(root,)| root)
        .map(|root| root.trim().to_string())
        .filter(|root| !root.is_empty())
        .ok_or_else(|| ApiError::invalid_input("group_workspace_root is required"))
}

async fn validate_initial_agents(
    pool: &SqlitePool,
    raw_ids: Option<&[String]>,
    owner_id: &str,
) -> Result<Vec<String>, ApiError> {
    let Some(raw_ids) = raw_ids else {
        return Ok(Vec::new());
    };

    let mut seen = BTreeSet::new();
    let mut ids = Vec::with_capacity(raw_ids.len());
    for raw_id in raw_ids {
        let id = validate_uuid(raw_id, "initial_agents")?;
        if !seen.insert(id.clone()) {
            return Err(ApiError::invalid_input(
                "initial_agents must not contain duplicates",
            ));
        }
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT owner_id, status FROM agents WHERE id = ?",
        )
        .bind(&id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))?;

        match row {
            None => return Err(ApiError::not_found("agent not found")),
            Some((_, status)) if status != "active" => {
                return Err(ApiError::not_found("agent not found"));
            }
            Some((owner, _)) if owner != owner_id => {
                return Err(ApiError::permission_denied("agent belongs to another user"));
            }
            Some(_) => ids.push(id),
        }
    }
    Ok(ids)
}

fn validate_note_title(raw: &str) -> Result<String, ApiError> {
    let title = raw.trim().to_string();
    let len = title.chars().count();
    if !(1..=200).contains(&len) {
        return Err(ApiError::invalid_input(
            "title must be between 1 and 200 characters",
        ));
    }
    Ok(title)
}

struct GroupFileUpload {
    filename: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
}

async fn read_group_file_part(mut multipart: Multipart) -> Result<GroupFileUpload, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::invalid_input("invalid multipart form-data"))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or_default().to_string();
        let mime_type = field.content_type().map(ToString::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|_| ApiError::invalid_input("invalid multipart file"))?
            .to_vec();
        return Ok(GroupFileUpload {
            filename,
            mime_type,
            bytes,
        });
    }
    Err(ApiError::invalid_input("file field is required"))
}

async fn read_group_workspace_file_part(
    mut multipart: Multipart,
) -> Result<GroupFileUpload, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::invalid_input("invalid multipart form-data"))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or_default().to_string();
        let mime_type = field.content_type().map(ToString::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|_| ApiError::invalid_input("invalid multipart file"))?
            .to_vec();
        if bytes.len() > MAX_WORKSPACE_UPLOAD_BYTES {
            return Err(ApiError::invalid_input(
                "uploaded file exceeds the workspace upload size limit",
            ));
        }
        return Ok(GroupFileUpload {
            filename,
            mime_type,
            bytes,
        });
    }
    Err(ApiError::invalid_input("file field is required"))
}

fn validate_group_file_name(raw: &str) -> Result<String, ApiError> {
    let filename = raw.trim().to_string();
    if filename.is_empty() {
        return Err(ApiError::invalid_input("upload filename is required"));
    }
    let normalized = filename.replace('\\', "/");
    if FsPath::new(&filename).is_absolute()
        || filename.starts_with('\\')
        || filename.starts_with('/')
        || normalized.starts_with("//")
        || filename.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || normalized.contains('/')
    {
        return Err(ApiError::invalid_input(
            "upload filename must be a plain filename",
        ));
    }
    Ok(filename)
}

fn validate_workspace_file_new_path(raw: &str) -> Result<String, ApiError> {
    let path = raw.trim().to_string();
    let len = path.chars().count();
    if !(1..=500).contains(&len) {
        return Err(ApiError::invalid_input(
            "new_path must be between 1 and 500 characters",
        ));
    }
    workspace_file_relative_path(&path)?;
    Ok(path)
}

fn workspace_file_relative_path(raw: &str) -> Result<Option<String>, ApiError> {
    let normalized = raw.trim().replace('\\', "/");
    if normalized.is_empty() {
        return Ok(None);
    }
    let mut chars = normalized.chars();
    if matches!(
        (chars.next(), chars.next()),
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic()
    ) {
        return Err(ApiError::invalid_input(
            "workspace file paths must be relative and stay inside the group workspace",
        ));
    }
    if normalized.starts_with('/') || normalized.starts_with("//") {
        return Err(ApiError::invalid_input(
            "workspace file paths must be relative and stay inside the group workspace",
        ));
    }
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == ".." || part == "~")
    {
        return Err(ApiError::invalid_input(
            "workspace file paths must be relative and stay inside the group workspace",
        ));
    }
    Ok(Some(normalized))
}

fn resolve_group_workspace_file_path(root: &FsPath, raw: &str) -> Result<PathBuf, ApiError> {
    match workspace_file_relative_path(raw)? {
        Some(relative) => resolve_workspace_path(root, &relative).map_err(workspace_path_error),
        None => Ok(root.to_path_buf()),
    }
}

/// Resolve an existing workspace entry without following its final component.
/// Parents are still canonicalized, so a symlinked parent cannot escape the
/// workspace, while deleting or moving a symlink removes the link itself.
fn resolve_group_workspace_entry_path(root: &FsPath, raw: &str) -> Result<PathBuf, ApiError> {
    let relative = workspace_file_relative_path(raw)?
        .ok_or_else(|| ApiError::invalid_input("cannot operate on the workspace root"))?;
    let relative_path = FsPath::new(&relative);
    let name = relative_path
        .file_name()
        .ok_or_else(|| ApiError::invalid_input("workspace path is invalid"))?;
    let parent_relative = relative_path.parent().unwrap_or_else(|| FsPath::new(""));
    let parent = if parent_relative.as_os_str().is_empty() {
        fs::canonicalize(root).map_err(|_| ApiError::invalid_input("workspace root is invalid"))?
    } else {
        resolve_workspace_path(root, &parent_relative.to_string_lossy())
            .map_err(workspace_path_error)?
    };
    if !parent.is_dir() {
        return Err(ApiError::invalid_input(
            "workspace path parent does not exist",
        ));
    }
    Ok(parent.join(name))
}

fn resolve_workspace_action_sources(
    root: &FsPath,
    raw_paths: &[String],
) -> Result<Vec<PathBuf>, ApiError> {
    if raw_paths.is_empty() {
        return Err(ApiError::invalid_input(
            "at least one workspace path is required",
        ));
    }
    if raw_paths.len() > MAX_WORKSPACE_ACTION_PATHS {
        return Err(ApiError::invalid_input("too many workspace paths"));
    }

    let mut sources = Vec::with_capacity(raw_paths.len());
    for raw in raw_paths {
        let source = resolve_group_workspace_entry_path(root, raw)?;
        if !path_exists_or_symlink(&source)? {
            return Err(ApiError::not_found("workspace path not found"));
        }
        if !sources.contains(&source) {
            sources.push(source);
        }
    }

    for (index, source) in sources.iter().enumerate() {
        if sources
            .iter()
            .enumerate()
            .any(|(other_index, other)| other_index != index && source.starts_with(other))
        {
            return Err(ApiError::invalid_input(
                "workspace paths must not contain one another",
            ));
        }
    }
    Ok(sources)
}

fn resolve_workspace_action_destination(root: &FsPath, raw: &str) -> Result<PathBuf, ApiError> {
    let destination = if raw.trim().is_empty() {
        fs::canonicalize(root).map_err(|_| ApiError::invalid_input("workspace root is invalid"))?
    } else {
        resolve_group_workspace_file_path(root, raw)?
    };
    if !destination.is_dir() {
        return Err(ApiError::invalid_input(
            "destination directory does not exist",
        ));
    }
    Ok(destination)
}

fn validate_action_destination(sources: &[PathBuf], destination: &FsPath) -> Result<(), ApiError> {
    for source in sources {
        let metadata = fs::symlink_metadata(source)
            .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        if !metadata_is_link_or_reparse(&metadata)
            && metadata.is_dir()
            && destination.starts_with(source)
        {
            return Err(ApiError::invalid_input(
                "cannot place a directory inside itself",
            ));
        }
    }
    Ok(())
}

fn move_destinations(sources: &[PathBuf], destination: &FsPath) -> Result<Vec<PathBuf>, ApiError> {
    let mut reserved = BTreeSet::new();
    let mut paths = Vec::with_capacity(sources.len());
    for source in sources {
        let name = source
            .file_name()
            .ok_or_else(|| ApiError::invalid_input("workspace path is invalid"))?;
        let target = destination.join(name);
        if path_exists_or_symlink(&target)? || !reserved.insert(target.clone()) {
            return Err(ApiError::conflict("destination already exists"));
        }
        paths.push(target);
    }
    Ok(paths)
}

fn copy_destinations(sources: &[PathBuf], destination: &FsPath) -> Result<Vec<PathBuf>, ApiError> {
    let mut reserved = BTreeSet::new();
    let mut paths = Vec::with_capacity(sources.len());
    for source in sources {
        let metadata = fs::symlink_metadata(source)
            .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        let name = source
            .file_name()
            .ok_or_else(|| ApiError::invalid_input("workspace path is invalid"))?
            .to_string_lossy();
        let direct = destination.join(name.as_ref());
        if !path_exists_or_symlink(&direct)? && reserved.insert(direct.clone()) {
            paths.push(direct);
            continue;
        }

        let mut available = None;
        for index in 1..=10_000 {
            let candidate = destination.join(copy_name(&name, metadata.is_dir(), index));
            if !path_exists_or_symlink(&candidate)? && reserved.insert(candidate.clone()) {
                available = Some(candidate);
                break;
            }
        }
        paths.push(available.ok_or_else(|| ApiError::conflict("no copy name is available"))?);
    }
    Ok(paths)
}

fn copy_name(name: &str, is_dir: bool, index: usize) -> String {
    let suffix = if index == 1 {
        " copy".to_string()
    } else {
        format!(" copy {index}")
    };
    if is_dir {
        return format!("{name}{suffix}");
    }
    let path = FsPath::new(name);
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| name.into());
    match path.extension() {
        Some(extension) => format!("{stem}{suffix}.{}", extension.to_string_lossy()),
        None => format!("{stem}{suffix}"),
    }
}

fn validate_copy_tree(path: &FsPath) -> Result<(), ApiError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    if metadata_is_link_or_reparse(&metadata) {
        return Err(ApiError::invalid_input("symbolic links cannot be copied"));
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(ApiError::invalid_input(
            "workspace path is not a file or directory",
        ));
    }
    for entry in fs::read_dir(path)
        .map_err(|_| ApiError::internal("failed to inspect workspace directory"))?
    {
        let entry =
            entry.map_err(|_| ApiError::internal("failed to inspect workspace directory"))?;
        validate_copy_tree(&entry.path())?;
    }
    Ok(())
}

fn copy_workspace_entry(source: &FsPath, destination: &FsPath) -> Result<(), ApiError> {
    let result = copy_workspace_entry_inner(source, destination);
    if result.is_err() && path_exists_or_symlink(destination).unwrap_or(false) {
        let _ = remove_workspace_entry(destination);
    }
    result
}

fn copy_workspace_entry_inner(source: &FsPath, destination: &FsPath) -> Result<(), ApiError> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    if metadata_is_link_or_reparse(&metadata) {
        return Err(ApiError::invalid_input("symbolic links cannot be copied"));
    }
    if metadata.is_file() {
        fs::copy(source, destination)
            .map(|_| ())
            .map_err(|_| ApiError::internal("failed to copy workspace file"))?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(ApiError::invalid_input(
            "workspace path is not a file or directory",
        ));
    }

    fs::create_dir(destination)
        .map_err(|_| ApiError::internal("failed to create workspace directory"))?;
    for entry in fs::read_dir(source)
        .map_err(|_| ApiError::internal("failed to read workspace directory"))?
    {
        let entry = entry.map_err(|_| ApiError::internal("failed to read workspace directory"))?;
        copy_workspace_entry_inner(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn clear_workspace_files(root: &FsPath) -> Result<(), ApiError> {
    let entries = fs::read_dir(root)
        .map_err(|_| ApiError::internal("failed to read workspace directory"))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|_| ApiError::internal("failed to read workspace directory"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for entry in entries {
        remove_workspace_entry(&entry)?;
    }
    Ok(())
}

fn remove_workspace_entry(path: &FsPath) -> Result<(), ApiError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ApiError::not_found("workspace path not found"))?;
    if metadata_is_link_or_reparse(&metadata) {
        fs::remove_file(path)
            .or_else(|_| fs::remove_dir(path))
            .map_err(|_| ApiError::internal("failed to delete workspace link"))?;
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|_| ApiError::internal("failed to delete workspace directory"))?;
    } else if metadata.is_file() {
        fs::remove_file(path).map_err(|_| ApiError::internal("failed to delete workspace file"))?;
    } else {
        return Err(ApiError::invalid_input(
            "workspace path is not a file or directory",
        ));
    }
    Ok(())
}

fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn workspace_file_response(
    path: &FsPath,
    root: &FsPath,
) -> Result<GroupWorkspaceFileResponse, ApiError> {
    let canonical =
        fs::canonicalize(path).map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    if !canonical.starts_with(root) {
        return Err(ApiError::invalid_input(
            "workspace file path escapes the group workspace",
        ));
    }
    let metadata =
        fs::metadata(path).map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|modified| OffsetDateTime::from(modified).format(&Rfc3339).ok());
    Ok(GroupWorkspaceFileResponse {
        path: display_workspace_path(root, path)?,
        name: workspace_file_name(path)?,
        is_dir: metadata.is_dir(),
        size: if metadata.is_dir() {
            None
        } else {
            Some(metadata.len() as i64)
        },
        modified_at,
        abs_path: path.to_string_lossy().to_string(),
    })
}

fn display_workspace_path(root: &FsPath, path: &FsPath) -> Result<String, ApiError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    if relative.as_os_str().is_empty() {
        return Ok(String::new());
    }
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn workspace_file_name(path: &FsPath) -> Result<String, ApiError> {
    Ok(path
        .file_name()
        .ok_or_else(|| ApiError::invalid_input("workspace path is invalid"))?
        .to_string_lossy()
        .to_string())
}

fn path_exists_or_symlink(path: &FsPath) -> Result<bool, ApiError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ApiError::invalid_input("workspace path is invalid")),
    }
}

fn group_file_relative_path(filename: &str) -> String {
    format!("{UPLOADS_DIR}/{filename}")
}

fn group_upload_path(root: &FsPath, filename: &str) -> Result<PathBuf, ApiError> {
    resolve_workspace_path(root, &group_file_relative_path(filename))
        .map_err(group_upload_path_safety_error)
}

fn group_upload_literal_path(root: &FsPath, filename: &str) -> PathBuf {
    root.join(UPLOADS_DIR).join(filename)
}

fn inspect_group_upload_dir(root: &FsPath) -> Result<(), ApiError> {
    let path = root.join(UPLOADS_DIR);
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ApiError::invalid_input(
                    "group uploads path must not be a symlink",
                ));
            }
            if !metadata.is_dir() {
                return Err(ApiError::invalid_input(
                    "group uploads path is not a directory",
                ));
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ApiError::invalid_input("group uploads path is invalid")),
    }
}

fn inspect_group_upload_file(root: &FsPath, filename: &str) -> Result<(), ApiError> {
    match fs::symlink_metadata(group_upload_literal_path(root, filename)) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ApiError::invalid_input(
                    "group upload path must not be a symlink",
                ));
            }
            if metadata.is_file() {
                return Err(ApiError::conflict(
                    "a file with this name already exists in uploads",
                ));
            }
            Err(ApiError::invalid_input("group upload path is not a file"))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ApiError::invalid_input("group upload path is invalid")),
    }
}

fn prepare_group_upload_path(root: &FsPath, filename: &str) -> Result<PathBuf, ApiError> {
    inspect_group_upload_dir(root)?;
    let path = group_upload_path(root, filename)?;
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid_input("group upload path is invalid"))?;
    fs::create_dir_all(parent)
        .map_err(|_| ApiError::invalid_input("group uploads path is not a directory"))?;
    inspect_group_upload_dir(root)?;
    let path = group_upload_path(root, filename)?;
    inspect_group_upload_file(root, filename)?;
    Ok(path)
}

fn unique_group_upload_filename(
    root: &FsPath,
    filename: String,
    allow_duplicate_name: bool,
) -> Result<String, ApiError> {
    if !allow_duplicate_name || !group_upload_literal_path(root, &filename).exists() {
        return Ok(filename);
    }

    let path = FsPath::new(&filename);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&filename);
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..=10_000 {
        let candidate = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        if !group_upload_literal_path(root, &candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(ApiError::conflict(
        "could not allocate a unique upload filename",
    ))
}

fn write_new_group_upload_file(path: &FsPath, bytes: &[u8]) -> Result<(), ApiError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == io::ErrorKind::AlreadyExists {
                ApiError::conflict("a file with this name already exists in uploads")
            } else {
                ApiError::internal("failed to write group file")
            }
        })?;
    file.write_all(bytes)
        .map_err(|_| ApiError::internal("failed to write group file"))
}

fn group_upload_path_safety_error(err: ToolError) -> ApiError {
    match err {
        ToolError::Invalid(message) => ApiError::invalid_input(message),
        ToolError::Io(_) => ApiError::invalid_input("group upload path is invalid"),
    }
}

fn workspace_path_error(err: ToolError) -> ApiError {
    match err {
        ToolError::Invalid(message) => ApiError::invalid_input(message),
        ToolError::Io(_) => ApiError::invalid_input("workspace path is invalid"),
    }
}

fn note_relative_path(note_id: &str) -> String {
    format!("{NOTES_DIR}/{note_id}{NOTE_FILE_SUFFIX}")
}

fn group_note_path(root: &FsPath, note_id: &str) -> Result<PathBuf, ApiError> {
    resolve_workspace_path(root, &note_relative_path(note_id)).map_err(path_safety_error)
}

fn group_note_literal_path(root: &FsPath, note_id: &str) -> PathBuf {
    root.join(note_relative_path(note_id))
}

enum GroupNoteFileState {
    Missing,
    File,
}

fn inspect_group_note_file(root: &FsPath, note_id: &str) -> Result<GroupNoteFileState, ApiError> {
    match fs::symlink_metadata(group_note_literal_path(root, note_id)) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ApiError::invalid_input(
                    "group note path must not be a symlink",
                ));
            }
            if !metadata.is_file() {
                return Err(ApiError::invalid_input("group note path is not a file"));
            }
            Ok(GroupNoteFileState::File)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(GroupNoteFileState::Missing),
        Err(_) => Err(ApiError::invalid_input("group note path is invalid")),
    }
}

fn read_group_note_content(
    root: &FsPath,
    note_id: &str,
    fallback: &str,
) -> Result<String, ApiError> {
    let path = group_note_path(root, note_id)?;
    match inspect_group_note_file(root, note_id)? {
        GroupNoteFileState::Missing => Ok(fallback.to_string()),
        GroupNoteFileState::File => fs::read_to_string(path)
            .map_err(|_| ApiError::invalid_input("group note is not valid UTF-8")),
    }
}

fn write_group_note_content(root: &FsPath, note_id: &str, content: &str) -> Result<(), ApiError> {
    let path = group_note_path(root, note_id)?;
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid_input("group notes path is invalid"))?;
    fs::create_dir_all(parent)
        .map_err(|_| ApiError::invalid_input("group notes path is not a directory"))?;

    let path = group_note_path(root, note_id)?;
    inspect_group_note_file(root, note_id)?;
    fs::write(path, content).map_err(|_| ApiError::internal("failed to write group note content"))
}

fn delete_group_note_content(root: &FsPath, note_id: &str) -> Result<(), ApiError> {
    let path = group_note_path(root, note_id)?;
    match inspect_group_note_file(root, note_id)? {
        GroupNoteFileState::Missing => Ok(()),
        GroupNoteFileState::File => fs::remove_file(path)
            .map_err(|_| ApiError::internal("failed to delete group note content")),
    }
}

fn path_safety_error(err: ToolError) -> ApiError {
    match err {
        ToolError::Invalid(message) => ApiError::invalid_input(message),
        ToolError::Io(_) => ApiError::invalid_input("group note path is invalid"),
    }
}

fn validate_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim().to_string();
    let len = name.chars().count();
    if !(1..=100).contains(&len) {
        return Err(ApiError::invalid_input(
            "name must be between 1 and 100 characters",
        ));
    }
    Ok(name)
}

fn validate_agent_mention_policy(raw: &str) -> Result<String, ApiError> {
    match raw.trim() {
        "display_only" => Ok("display_only".to_string()),
        "bounded_schedule" => Ok("bounded_schedule".to_string()),
        _ => Err(ApiError::invalid_input(
            "agent_mention_policy must be display_only or bounded_schedule",
        )),
    }
}

fn validate_scheduler_minimum(
    field: &'static str,
    value: i64,
    minimum: i64,
) -> Result<(), ApiError> {
    if value < minimum {
        return Err(ApiError::invalid_input(format!(
            "{field} must be >= {minimum}"
        )));
    }
    Ok(())
}

async fn validate_moderator_provider(
    pool: &SqlitePool,
    raw_id: &str,
    owner_id: &str,
) -> Result<String, ApiError> {
    let id = validate_uuid(raw_id, "moderator_provider_id")?;
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT owner_id, status FROM llm_providers WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::invalid_input(
            "moderator_provider_id does not reference a provider",
        )),
        Some((_, status)) if status != "active" => Err(ApiError::invalid_input(
            "moderator_provider_id is not active",
        )),
        Some((owner, _)) if owner != owner_id => Err(ApiError::permission_denied(
            "provider belongs to another user",
        )),
        Some(_) => Ok(id),
    }
}

fn validate_proactive_max_rounds(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(1);
    if !(1..=5).contains(&value) {
        return Err(ApiError::invalid_input(
            "proactive_max_rounds must be between 1 and 5",
        ));
    }
    Ok(value)
}

fn validate_multiplier(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(1);
    if value < 1 {
        return Err(ApiError::invalid_input(
            "proactive_reply_multiplier must be >= 1",
        ));
    }
    Ok(value)
}

fn validate_agent_free_mention_max_dispatches(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(8);
    if value < 0 {
        return Err(ApiError::invalid_input(
            "agent_free_mention_max_dispatches must be >= 0",
        ));
    }
    Ok(value)
}

fn validate_communication_mode(raw: Option<&str>) -> Result<String, ApiError> {
    let mode = raw
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .unwrap_or("mesh");
    match mode {
        "mesh" | "star" | "hierarchical" | "ring" => Ok(mode.to_string()),
        _ => Err(ApiError::invalid_input(
            "communication_mode must be one of mesh, star, hierarchical, or ring",
        )),
    }
}

fn normalize_description(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
}

/// Seed the topology fields for the `position`-th agent added while creating a
/// group. The first agent takes the role every mode needs at least one of, so a
/// freshly created group always has a valid topology.
fn initial_agent_topology(mode: &str, position: usize) -> (Option<&'static str>, Option<i64>) {
    match mode {
        "star" if position == 0 => (Some("hub"), None),
        "hierarchical" if position == 0 => (Some("leader"), None),
        "hierarchical" => (Some("worker"), None),
        "ring" => (None, Some(position as i64 + 1)),
        _ => (None, None),
    }
}

fn parse_json_list(raw: Option<&str>) -> Option<Vec<String>> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

fn now_plus_rfc3339(microseconds: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::microseconds(microseconds))
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}
