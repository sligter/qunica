use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{
    auth::current_user_id,
    error::ApiError,
    workspace_files::{
        self, ConversationScope, SaveWorkspaceFileTextRequest, WorkspaceFilePathQuery,
    },
    AppState,
};
use crate::runtime::workspace_scope::WorkspaceMode;

const SELECT_DIRECT_CHAT: &str = "SELECT g.id, g.name AS title, g.title_source, \
    g.direct_agent_id AS agent_id, a.name AS agent_name, a.status AS agent_status, \
    a.workspace_id AS workspace_id, g.status, g.created_at, g.updated_at \
    FROM groups g LEFT JOIN agents a ON a.id = g.direct_agent_id \
    WHERE g.id = ? AND g.owner_id = ? AND g.status = 'active' \
    AND g.conversation_kind = 'direct'";

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    agent_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    title: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DirectChatResponse {
    id: String,
    title: String,
    title_source: String,
    agent_id: Option<String>,
    agent_name: Option<String>,
    agent_status: Option<String>,
    workspace_id: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct OwnedAgent {
    id: String,
    name: String,
    workspace_id: Option<String>,
    status: String,
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<DirectChatResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let agent_id = validate_uuid(&body.agent_id, "agent_id")?;
    let agent = sqlx::query_as::<_, OwnedAgent>(
        "SELECT id, name, workspace_id, status FROM agents WHERE id = ? AND owner_id = ?",
    )
    .bind(&agent_id)
    .bind(&owner_id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("agent not found"))?;
    if agent.status != "active" {
        return Err(ApiError::conflict("agent is unavailable"));
    }
    let language: Option<String> =
        sqlx::query_scalar("SELECT language FROM system_settings WHERE owner_id = ? LIMIT 1")
            .bind(&owner_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(|_| ApiError::internal("database error"))?;
    let title = direct_chat_title(language.as_deref(), &agent.name);
    let id = insert_direct_chat(
        state.db.pool(),
        &owner_id,
        &agent.id,
        agent.workspace_id.as_deref(),
        &title,
        WorkspaceMode::default(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(fetch(state.db.pool(), &id, &owner_id).await?),
    ))
}

/// Insert the four rows a direct chat is made of, in one transaction.
///
/// Shared with the built-in Assistant, which is an ordinary direct chat over an
/// ordinary agent row. Duplicating these inserts there would be a second place
/// for the conversation shape to drift.
pub(crate) async fn insert_direct_chat(
    pool: &SqlitePool,
    owner_id: &str,
    agent_id: &str,
    workspace_id: Option<&str>,
    title: &str,
    workspace_mode: WorkspaceMode,
) -> Result<String, ApiError> {
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start direct chat transaction"))?;
    sqlx::query("INSERT INTO groups (id, owner_id, workspace_id, name, free_speech, proactive_mode, scheduler_enabled, conversation_kind, direct_agent_id, title_source, status, created_at, updated_at) VALUES (?, ?, ?, ?, 1, 0, 0, 'direct', ?, 'automatic', 'active', ?, ?)")
        .bind(&id).bind(owner_id).bind(workspace_id).bind(title).bind(agent_id).bind(&now).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to create direct chat"))?;
    sqlx::query("INSERT INTO group_members (group_id, user_id, role, status, joined_at) VALUES (?, ?, 'owner', 'active', ?)")
        .bind(&id).bind(owner_id).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to create direct chat membership"))?;
    // The conversation workspace is a copy of the agent's own binding, so the
    // default `group` mode points the agent right back at its own directory.
    let context_scope_json = workspace_mode
        .to_context_scope(None)
        .map_err(|_| ApiError::internal("failed to serialize context scope"))?;
    sqlx::query("INSERT INTO group_agents (group_id, agent_id, response_mode, context_scope_json, status, joined_at, updated_at) VALUES (?, ?, 'default', ?, 'active', ?, ?)")
        .bind(&id).bind(agent_id).bind(&context_scope_json).bind(&now).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to bind direct chat agent"))?;
    sqlx::query("INSERT INTO threads (id, group_id, agent_id, status, next_seq, created_at, updated_at) VALUES (?, ?, NULL, 'active', 1, ?, ?)")
        .bind(Uuid::new_v4().to_string()).bind(&id).bind(&now).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to create direct chat thread"))?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit direct chat"))?;
    Ok(id)
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DirectChatResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    // The Assistant's chat is reached through the floating dock, never the chat
    // list; showing it there would put a conversation the user cannot delete
    // alongside ones they can.
    let rows = sqlx::query_as::<_, DirectChatResponse>("SELECT g.id, g.name AS title, g.title_source, g.direct_agent_id AS agent_id, a.name AS agent_name, a.status AS agent_status, a.workspace_id AS workspace_id, g.status, g.created_at, g.updated_at FROM groups g LEFT JOIN agents a ON a.id = g.direct_agent_id WHERE g.owner_id = ? AND g.status = 'active' AND g.conversation_kind = 'direct' AND COALESCE(a.is_system, 0) = 0 ORDER BY g.updated_at DESC, g.id DESC")
        .bind(&owner_id).fetch_all(state.db.pool()).await.map_err(|_| ApiError::internal("database error"))?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
) -> Result<Json<DirectChatResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(
        fetch(
            state.db.pool(),
            &validate_uuid(&chat_id, "chat id")?,
            &owner_id,
        )
        .await?,
    ))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<DirectChatResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    let title = validate_title(&body.title)?;
    fetch(state.db.pool(), &chat_id, &owner_id).await?;
    sqlx::query("UPDATE groups SET name = ?, title_source = 'manual', updated_at = ? WHERE id = ? AND owner_id = ? AND status = 'active' AND conversation_kind = 'direct'")
        .bind(&title).bind(now_rfc3339()).bind(&chat_id).bind(&owner_id).execute(state.db.pool()).await
        .map_err(|_| ApiError::internal("failed to update direct chat"))?;
    Ok(Json(fetch(state.db.pool(), &chat_id, &owner_id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    fetch(state.db.pool(), &chat_id, &owner_id).await?;
    sqlx::query("UPDATE groups SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ? AND conversation_kind = 'direct'")
        .bind(now_rfc3339()).bind(&chat_id).bind(&owner_id).execute(state.db.pool()).await
        .map_err(|_| ApiError::internal("failed to delete direct chat"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Read-only workspace file APIs for direct chats.  Upload, rename and delete
/// remain group-only operations; text saves are the sole direct-chat mutation.
pub async fn get_workspace_root(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
) -> Result<Json<workspace_files::WorkspaceRootResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    Ok(Json(
        workspace_files::workspace_root(
            state.db.pool(),
            workspace_files::ConversationRoot::conversation(
                ConversationScope::DirectChats,
                &chat_id,
                &owner_id,
            ),
        )
        .await?,
    ))
}

pub async fn list_workspace_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
) -> Result<Json<Vec<workspace_files::WorkspaceFileResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    Ok(Json(
        workspace_files::list_workspace_files(
            state.db.pool(),
            workspace_files::ConversationRoot::from_query(
                ConversationScope::DirectChats,
                &chat_id,
                &owner_id,
                query.agent_id(),
            ),
            &query.path,
        )
        .await?,
    ))
}

pub async fn preview_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
) -> Result<Json<workspace_files::WorkspaceFilePreviewResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    Ok(Json(
        workspace_files::preview_workspace_file(
            state.db.pool(),
            workspace_files::ConversationRoot::from_query(
                ConversationScope::DirectChats,
                &chat_id,
                &owner_id,
                query.agent_id(),
            ),
            &query.path,
        )
        .await?,
    ))
}

pub async fn download_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
) -> Result<axum::response::Response, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    workspace_files::stream_workspace_file(
        state.db.pool(),
        workspace_files::ConversationRoot::from_query(
            ConversationScope::DirectChats,
            &chat_id,
            &owner_id,
            query.agent_id(),
        ),
        &query.path,
    )
    .await
}

pub async fn read_workspace_file_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
) -> Result<Json<workspace_files::WorkspaceFileTextResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    Ok(Json(
        workspace_files::read_workspace_file_text(
            state.db.pool(),
            workspace_files::ConversationRoot::from_query(
                ConversationScope::DirectChats,
                &chat_id,
                &owner_id,
                query.agent_id(),
            ),
            &query.path,
        )
        .await?,
    ))
}

pub async fn save_workspace_file_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
    Json(body): Json<SaveWorkspaceFileTextRequest>,
) -> Result<Json<workspace_files::WorkspaceFileTextResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    Ok(Json(
        workspace_files::save_workspace_file_text(
            state.db.pool(),
            workspace_files::ConversationRoot::from_query(
                ConversationScope::DirectChats,
                &chat_id,
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

async fn fetch(
    pool: &SqlitePool,
    id: &str,
    owner_id: &str,
) -> Result<DirectChatResponse, ApiError> {
    sqlx::query_as(SELECT_DIRECT_CHAT)
        .bind(id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))?
        .ok_or_else(|| ApiError::not_found("direct chat not found"))
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
}

fn validate_title(raw: &str) -> Result<String, ApiError> {
    let title = raw.trim();
    if !(1..=120).contains(&title.chars().count()) {
        return Err(ApiError::invalid_input(
            "title must be between 1 and 120 characters",
        ));
    }
    Ok(title.to_string())
}

fn direct_chat_title(language: Option<&str>, agent_name: &str) -> String {
    if language == Some("zh-CN") {
        format!("\u{4e0e} {agent_name} \u{7684}\u{65b0}\u{5bf9}\u{8bdd}")
    } else {
        format!("New chat with {agent_name}")
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

// Direct chats get the same file mutations groups have. A one-on-one chat is
// where a user is most likely to just drop a file in, so read-only would be the
// wrong asymmetry.
pub async fn upload_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<crate::api::groups::GroupWorkspaceUploadQuery>,
    multipart: axum::extract::Multipart,
) -> Result<
    (
        StatusCode,
        Json<crate::api::groups::GroupWorkspaceFileResponse>,
    ),
    ApiError,
> {
    crate::api::groups::upload_group_workspace_file(
        state,
        headers,
        ConversationScope::DirectChats,
        chat_id,
        query,
        multipart,
    )
    .await
}

pub async fn rename_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
    Json(body): Json<crate::api::groups::GroupWorkspaceFileRenameRequest>,
) -> Result<Json<crate::api::groups::GroupWorkspaceFileResponse>, ApiError> {
    crate::api::groups::rename_group_workspace_file(
        state,
        headers,
        ConversationScope::DirectChats,
        chat_id,
        query,
        body,
    )
    .await
}

pub async fn delete_workspace_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
) -> Result<StatusCode, ApiError> {
    crate::api::groups::delete_group_workspace_file(
        state,
        headers,
        ConversationScope::DirectChats,
        chat_id,
        query,
    )
    .await
}

pub async fn workspace_file_actions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<WorkspaceFilePathQuery>,
    Json(body): Json<crate::api::groups::GroupWorkspaceFileActionRequest>,
) -> Result<StatusCode, ApiError> {
    crate::api::groups::act_on_group_workspace_files(
        state,
        headers,
        ConversationScope::DirectChats,
        chat_id,
        query,
        body,
    )
    .await
}

pub async fn list_workspace_roots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
) -> Result<Json<Vec<workspace_files::ConversationRootEntry>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let chat_id = validate_uuid(&chat_id, "chat id")?;
    Ok(Json(
        workspace_files::list_conversation_roots(
            state.db.pool(),
            ConversationScope::DirectChats,
            &chat_id,
            &owner_id,
        )
        .await?,
    ))
}
