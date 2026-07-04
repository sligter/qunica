use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    path::{Path as FsPath, PathBuf},
    process::{Command as StdCommand, Stdio},
    time::Duration as StdDuration,
};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::process::tokio_command_no_window;
use crate::tools::{resolve_workspace_path, ToolError};

const GROUP_COLUMNS: &str = "id, owner_id, workspace_id, name, description, announcement, \
     free_speech, proactive_mode, proactive_max_rounds, proactive_reply_multiplier, \
     allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
     muted_agent_ids_json, admin_agent_ids_json, muted_member_ids_json, status, \
     created_at, updated_at";

const GROUP_AGENT_COLUMNS: &str = "group_agents.group_id, group_agents.agent_id, \
     group_agents.display_name, agents.name AS agent_name, group_agents.role, \
     group_agents.topology_role, group_agents.speaking_order, group_agents.response_mode, \
     group_agents.context_scope_json, group_agents.status, group_agents.joined_at";

const GROUP_MEMBER_COLUMNS: &str = "group_members.group_id, group_members.user_id, \
     users.name AS user_name, group_members.role, group_members.status, \
     group_members.joined_at";

const GROUP_NOTE_COLUMNS: &str = "id, group_id, title, content, created_at, updated_at";

const NOTES_DIR: &str = "Notes";
const NOTE_FILE_SUFFIX: &str = ".md";
const GROUP_FILE_COLUMNS: &str = "id, group_id, filename, file_size, mime_type, created_at";
const UPLOADS_DIR: &str = "uploads";
const MAX_WORKSPACE_PREVIEW_BYTES: usize = 64 * 1024;
const TEXT_WORKSPACE_PREVIEW_CHARS: usize = 20_000;
const MAX_WORKSPACE_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const MAX_GIT_OUTPUT_CHARS: usize = 8_000;
const GIT_COMMAND_TIMEOUT_SECONDS: u64 = 120;
const BINARY_PREVIEW_MESSAGE: &str = "Preview is not available for binary or unsupported files.";

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
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentAddRequest {
    agent_id: String,
    #[serde(default)]
    share_group_workspace: Option<bool>,
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
    share_group_workspace: bool,
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
pub struct GroupWorkspaceFilePathQuery {
    #[serde(default)]
    path: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceFileRenameRequest {
    new_path: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitPathsRequest {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupWorkspaceGitCommitRequest {
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
    muted_agent_ids: Option<Vec<String>>,
    admin_agent_ids: Option<Vec<String>>,
    muted_member_ids: Option<Vec<String>>,
    status: String,
    created_at: String,
    updated_at: String,
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

#[derive(Debug, Serialize)]
pub struct GroupWorkspaceGitFileStatusResponse {
    path: String,
    status: String,
    staged: bool,
    unstaged: bool,
}

#[derive(Debug, Serialize)]
pub struct GroupWorkspaceGitStatusResponse {
    available: bool,
    branch: Option<String>,
    clean: bool,
    files: Vec<GroupWorkspaceGitFileStatusResponse>,
    message: Option<String>,
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
    muted_agent_ids_json: Option<String>,
    admin_agent_ids_json: Option<String>,
    muted_member_ids_json: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
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
        let share_group_workspace = group_workspace_shared(row.context_scope_json.as_deref());
        Self {
            id,
            group_id: row.group_id,
            agent_id: row.agent_id,
            display_name: row.display_name.unwrap_or(row.agent_name),
            role: row.role,
            topology_role: row.topology_role,
            speaking_order: row.speaking_order,
            response_mode: row.response_mode,
            share_group_workspace,
            context_usage: None,
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
          status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?)",
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
        .bind(r#"{"share_group_workspace":true}"#)
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
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<GroupResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let sql = format!(
        "SELECT {GROUP_COLUMNS} FROM groups \
         WHERE owner_id = ? AND status = 'active' \
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
    let group_id = validate_uuid(&group_id, "group id")?;

    let existing = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;

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
         communication_mode = ?, updated_at = ? \
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
    Ok(Json(row.into()))
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

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    Ok(Json(GroupWorkspaceRootResponse {
        root: root.to_string_lossy().to_string(),
        separator: std::path::MAIN_SEPARATOR.to_string(),
    }))
}

pub async fn list_group_workspace_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceFilePathQuery>,
) -> Result<Json<Vec<GroupWorkspaceFileResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let directory = resolve_group_workspace_file_path(&root, &query.path)?;
    if !directory.is_dir() {
        return Err(ApiError::invalid_input("workspace path is not a directory"));
    }

    let mut rows = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|_| ApiError::invalid_input("workspace path is not a directory"))?
    {
        let entry = entry.map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        rows.push(workspace_file_response(&entry.path(), &root)?);
    }
    rows.sort_by(|left, right| {
        (if left.is_dir { 0 } else { 1 }, left.name.to_lowercase())
            .cmp(&(if right.is_dir { 0 } else { 1 }, right.name.to_lowercase()))
    });
    Ok(Json(rows))
}

pub async fn preview_group_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceFilePathQuery>,
) -> Result<Json<GroupWorkspaceFilePreviewResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let file_path = resolve_group_workspace_file_path(&root, &query.path)?;
    if !file_path.is_file() {
        return Err(ApiError::invalid_input("workspace path is not a file"));
    }

    let metadata = fs::metadata(&file_path)
        .map_err(|_| ApiError::invalid_input("workspace path is not a file"))?;
    let size = metadata.len() as i64;
    let mut file = fs::File::open(&file_path)
        .map_err(|_| ApiError::invalid_input("workspace path is not a file"))?;
    let mut sample = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_WORKSPACE_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut sample)
        .map_err(|_| ApiError::invalid_input("workspace file could not be read"))?;

    let byte_truncated = sample.len() > MAX_WORKSPACE_PREVIEW_BYTES;
    let capped = &sample[..sample.len().min(MAX_WORKSPACE_PREVIEW_BYTES)];

    if !workspace_file_looks_text(&file_path, capped) {
        return Ok(Json(GroupWorkspaceFilePreviewResponse {
            path: display_workspace_path(&root, &file_path)?,
            name: workspace_file_name(&file_path)?,
            is_text: false,
            content: None,
            truncated: false,
            message: Some(BINARY_PREVIEW_MESSAGE.to_string()),
            size: Some(size),
        }));
    }

    let mut content = String::from_utf8_lossy(capped).to_string();
    let mut truncated = byte_truncated;
    if content.chars().count() > TEXT_WORKSPACE_PREVIEW_CHARS {
        content = content.chars().take(TEXT_WORKSPACE_PREVIEW_CHARS).collect();
        truncated = true;
    }

    Ok(Json(GroupWorkspaceFilePreviewResponse {
        path: display_workspace_path(&root, &file_path)?,
        name: workspace_file_name(&file_path)?,
        is_text: true,
        content: Some(content),
        truncated,
        message: None,
        size: Some(size),
    }))
}

pub async fn upload_group_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<GroupWorkspaceFileResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let upload = read_group_workspace_file_part(multipart).await?;
    let filename = validate_group_file_name(&upload.filename)?;
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
    Query(query): Query<GroupWorkspaceFilePathQuery>,
) -> Result<Response, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let file_path = resolve_group_workspace_file_path(&root, &query.path)?;
    if !file_path.is_file() {
        return Err(ApiError::invalid_input("workspace path is not a file"));
    }
    let bytes = fs::read(&file_path)
        .map_err(|_| ApiError::invalid_input("workspace file could not be read"))?;
    let filename = workspace_file_name(&file_path)?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(workspace_file_content_type(&file_path)),
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            header_safe_filename(&filename)
        ))
        .map_err(|_| ApiError::internal("failed to build download headers"))?,
    );
    Ok((response_headers, bytes).into_response())
}

pub async fn rename_group_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceFilePathQuery>,
    Json(body): Json<GroupWorkspaceFileRenameRequest>,
) -> Result<Json<GroupWorkspaceFileResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
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
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceFilePathQuery>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let target = resolve_group_workspace_file_path(&root, &query.path)?;
    if target == root {
        return Err(ApiError::invalid_input("cannot delete the workspace root"));
    }
    if target.is_dir() {
        fs::remove_dir(&target).map_err(|err| {
            if err.kind() == io::ErrorKind::DirectoryNotEmpty {
                ApiError::invalid_input("directory must be empty before it can be deleted")
            } else {
                ApiError::internal("failed to delete workspace directory")
            }
        })?;
    } else if target.is_file() {
        fs::remove_file(&target)
            .map_err(|_| ApiError::internal("failed to delete workspace file"))?;
    } else {
        return Err(ApiError::invalid_input(
            "workspace path is not a file or directory",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_group_workspace_git_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupWorkspaceGitStatusResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    Ok(Json(group_workspace_git_status(&root).await?))
}

pub async fn stage_group_workspace_git_paths(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitPathsRequest>,
) -> Result<Json<GroupWorkspaceGitStatusResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let paths = validate_git_paths(&root, &body.paths)?;
    let args = if paths.is_empty() {
        git_args(&["add", "-A"])
    } else {
        git_args_with_paths(&["add", "--"], &paths)
    };
    run_git_or_api_error(&root, &args, "git stage failed").await?;
    Ok(Json(group_workspace_git_status(&root).await?))
}

pub async fn unstage_group_workspace_git_paths(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitPathsRequest>,
) -> Result<Json<GroupWorkspaceGitStatusResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let paths = validate_git_paths(&root, &body.paths)?;
    let args = if paths.is_empty() {
        git_args(&["reset", "--", "."])
    } else {
        git_args_with_paths(&["reset", "--"], &paths)
    };
    run_git_or_api_error(&root, &args, "git unstage failed").await?;
    Ok(Json(group_workspace_git_status(&root).await?))
}

pub async fn commit_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupWorkspaceGitCommitRequest>,
) -> Result<Json<GroupWorkspaceGitStatusResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let message = validate_git_commit_message(&body.message)?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    let args = vec!["commit".to_string(), "-m".to_string(), message];
    run_git_or_api_error(&root, &args, "git commit failed").await?;
    Ok(Json(group_workspace_git_status(&root).await?))
}

pub async fn pull_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupWorkspaceGitStatusResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    run_git_or_api_error(&root, &git_args(&["pull", "--ff-only"]), "git pull failed").await?;
    Ok(Json(group_workspace_git_status(&root).await?))
}

pub async fn push_group_workspace_git(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupWorkspaceGitStatusResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let root = group_files_workspace_root(state.db.pool(), &group, &owner_id).await?;
    run_git_or_api_error(&root, &git_args(&["push"]), "git push failed").await?;
    Ok(Json(group_workspace_git_status(&root).await?))
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
    let group_id = validate_uuid(&group_id, "group id")?;
    let user_id = validate_uuid(&body.user_id, "user_id")?;
    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
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
    Ok((
        StatusCode::CREATED,
        Json(row.into_response(&muted_member_ids)),
    ))
}

pub async fn remove_group_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let user_id = validate_uuid(&user_id, "user id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
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

    Ok(StatusCode::NO_CONTENT)
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
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&body.agent_id, "agent_id")?;
    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    validate_owned_active_agent(state.db.pool(), &agent_id, &owner_id).await?;

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
    let context_scope_json = context_scope_with_group_workspace(
        existing_context_scope,
        body.share_group_workspace.unwrap_or(true),
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
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn remove_group_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
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
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group agent removal"))?;

    Ok(StatusCode::NO_CONTENT)
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

    let context_scope_json = context_scope_with_group_workspace(
        existing.context_scope_json.as_deref(),
        body.share_group_workspace,
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

async fn fetch_row(pool: &SqlitePool, group_id: &str) -> Result<Option<GroupRow>, ApiError> {
    let sql = format!("SELECT {GROUP_COLUMNS} FROM groups WHERE id = ?");
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

async fn group_workspace_git_status(
    root: &FsPath,
) -> Result<GroupWorkspaceGitStatusResponse, ApiError> {
    let args = git_args(&[
        "-c",
        "core.quotePath=false",
        "status",
        "--porcelain=v1",
        "-b",
    ]);
    match run_git_command(root, &args).await {
        Ok(output) if output.success => Ok(parse_git_status(&output.stdout)),
        Ok(output) if git_output_is_not_repository(&output) => {
            Ok(unavailable_git_status("workspace is not a Git repository"))
        }
        Ok(output) => Ok(unavailable_git_status(format_git_failure(
            "git status failed",
            &output,
        ))),
        Err(GitCommandError::MissingGit) => {
            Ok(unavailable_git_status("git executable was not found"))
        }
        Err(err) => Ok(unavailable_git_status(git_command_error_message(err))),
    }
}

fn unavailable_git_status(message: impl Into<String>) -> GroupWorkspaceGitStatusResponse {
    GroupWorkspaceGitStatusResponse {
        available: false,
        branch: None,
        clean: true,
        files: Vec::new(),
        message: Some(message.into()),
    }
}

fn parse_git_status(stdout: &str) -> GroupWorkspaceGitStatusResponse {
    let mut branch = None;
    let mut files = Vec::new();
    for line in stdout.lines() {
        if let Some(summary) = line.strip_prefix("## ") {
            branch = parse_git_branch(summary);
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let status = line[..2].to_string();
        let mut path = line[3..].to_string();
        if let Some((_, renamed_to)) = path.rsplit_once(" -> ") {
            path = renamed_to.to_string();
        }
        let bytes = status.as_bytes();
        let staged = bytes
            .first()
            .is_some_and(|value| *value != b' ' && *value != b'?');
        let unstaged = bytes.get(1).is_some_and(|value| *value != b' ');
        files.push(GroupWorkspaceGitFileStatusResponse {
            path,
            status,
            staged,
            unstaged,
        });
    }

    GroupWorkspaceGitStatusResponse {
        available: true,
        branch,
        clean: files.is_empty(),
        files,
        message: None,
    }
}

fn parse_git_branch(summary: &str) -> Option<String> {
    let branch = summary
        .split("...")
        .next()
        .unwrap_or(summary)
        .trim()
        .strip_prefix("No commits yet on ")
        .unwrap_or_else(|| summary.split("...").next().unwrap_or(summary).trim())
        .trim();
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
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

fn git_args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn git_args_with_paths(prefix: &[&str], paths: &[String]) -> Vec<String> {
    let mut args = git_args(prefix);
    args.extend(paths.iter().cloned());
    args
}

async fn run_git_or_api_error(
    root: &FsPath,
    args: &[String],
    context: &'static str,
) -> Result<GitCommandOutput, ApiError> {
    let output = run_git_command(root, args)
        .await
        .map_err(|err| ApiError::invalid_input(git_command_error_message(err)))?;
    if output.success {
        Ok(output)
    } else {
        Err(ApiError::invalid_input(format_git_failure(
            context, &output,
        )))
    }
}

#[derive(Debug)]
struct GitCommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
enum GitCommandError {
    MissingGit,
    TimedOut,
    Io(&'static str),
}

async fn run_git_command(
    root: &FsPath,
    args: &[String],
) -> Result<GitCommandOutput, GitCommandError> {
    let mut child = git_command(root, args).spawn().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            GitCommandError::MissingGit
        } else {
            GitCommandError::Io("failed to start git command")
        }
    })?;

    let mut stdout_handle = child.stdout.take().expect("stdout was piped");
    let mut stderr_handle = child.stderr.take().expect("stderr was piped");
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let wait = async {
        let (stdout_result, stderr_result, status_result) = tokio::join!(
            stdout_handle.read_to_end(&mut stdout_buf),
            stderr_handle.read_to_end(&mut stderr_buf),
            child.wait(),
        );
        stdout_result.map_err(|_| GitCommandError::Io("failed to read git stdout"))?;
        stderr_result.map_err(|_| GitCommandError::Io("failed to read git stderr"))?;
        status_result.map_err(|_| GitCommandError::Io("failed to wait for git command"))
    };

    match timeout(StdDuration::from_secs(GIT_COMMAND_TIMEOUT_SECONDS), wait).await {
        Ok(status) => {
            let status = status?;
            Ok(GitCommandOutput {
                success: status.success(),
                stdout: truncate_git_output(&String::from_utf8_lossy(&stdout_buf)),
                stderr: truncate_git_output(&String::from_utf8_lossy(&stderr_buf)),
            })
        }
        Err(_) => {
            let _ = child.start_kill();
            Err(GitCommandError::TimedOut)
        }
    }
}

fn git_command(root: &FsPath, args: &[String]) -> Command {
    let mut command = StdCommand::new("git");
    command
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut command = tokio_command_no_window(command);
    command.kill_on_drop(true);
    command
}

fn git_command_error_message(err: GitCommandError) -> String {
    match err {
        GitCommandError::MissingGit => "git executable was not found".to_string(),
        GitCommandError::TimedOut => {
            format!("git command timed out after {GIT_COMMAND_TIMEOUT_SECONDS} seconds")
        }
        GitCommandError::Io(message) => message.to_string(),
    }
}

fn git_output_is_not_repository(output: &GitCommandOutput) -> bool {
    let combined = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    combined.contains("not a git repository")
}

fn format_git_failure(context: &str, output: &GitCommandOutput) -> String {
    let details = [output.stderr.trim(), output.stdout.trim()]
        .into_iter()
        .find(|part| !part.is_empty())
        .unwrap_or("command exited with a non-zero status");
    format!("{context}: {details}")
}

fn truncate_git_output(output: &str) -> String {
    if output.chars().count() <= MAX_GIT_OUTPUT_CHARS {
        return output.to_string();
    }
    let truncated: String = output.chars().take(MAX_GIT_OUTPUT_CHARS).collect();
    format!("{truncated}\n[output truncated]")
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
        "hierarchical" => Ok((Some("worker".to_string()), None)),
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

fn group_workspace_shared(raw: Option<&str>) -> bool {
    raw.and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| value.get("share_group_workspace").and_then(Value::as_bool))
        == Some(true)
}

fn context_scope_with_group_workspace(
    raw: Option<&str>,
    share: bool,
) -> Result<Option<String>, ApiError> {
    let mut object = raw
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| match value {
            Value::Object(object) => Some(object),
            _ => None,
        })
        .unwrap_or_default();

    if share {
        object.insert("share_group_workspace".to_string(), Value::Bool(true));
    } else {
        object.remove("share_group_workspace");
    }

    if object.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(&Value::Object(object))
            .map(Some)
            .map_err(|_| ApiError::internal("failed to serialize context scope"))
    }
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
    rows.iter()
        .map(|row| {
            let role = match row.topology_role.as_deref() {
                Some("leader") => "leader",
                Some("worker") => "worker",
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

async fn create_group_workspace(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
    group_name: &str,
    now: &str,
) -> Result<String, ApiError> {
    let root = require_group_workspace_root(pool, owner_id).await?;
    let storage_dir = PathBuf::from(root).join(group_id);
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
    .bind(format!("group:{group_name}"))
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

fn workspace_file_looks_text(path: &FsPath, sample: &[u8]) -> bool {
    if sample.contains(&0) {
        return false;
    }
    if workspace_file_has_text_extension(path) {
        return true;
    }
    std::str::from_utf8(sample).is_ok()
}

fn workspace_file_has_text_extension(path: &FsPath) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "json"
            | "jsonl"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "cfg"
            | "log"
            | "xml"
            | "html"
            | "css"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "py"
            | "sh"
            | "bat"
            | "ps1"
            | "sql"
            | "rst"
    )
}

fn workspace_file_content_type(path: &FsPath) -> &'static str {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return "application/octet-stream";
    };
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "log" | "csv" | "md" | "markdown" | "rst" => "text/plain",
        "html" => "text/html",
        "css" => "text/css",
        "js" | "jsx" => "text/javascript",
        "json" | "jsonl" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" | "ini" | "cfg" => "text/plain",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn header_safe_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|ch| {
            if ch.is_ascii() && ch != '"' && ch != '\\' && !ch.is_control() {
                ch
            } else {
                '_'
            }
        })
        .collect()
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

fn initial_agent_topology(mode: &str, position: usize) -> (Option<&'static str>, Option<i64>) {
    match mode {
        "star" if position == 0 => (Some("hub"), None),
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
