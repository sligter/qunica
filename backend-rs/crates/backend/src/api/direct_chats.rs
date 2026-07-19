use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const SELECT_DIRECT_CHAT: &str = "SELECT g.id, g.name AS title, g.title_source, \
    g.direct_agent_id AS agent_id, a.name AS agent_name, a.status AS agent_status, \
    g.workspace_id, g.status, g.created_at, g.updated_at \
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
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let title = direct_chat_title(language.as_deref(), &agent.name);
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start direct chat transaction"))?;
    sqlx::query("INSERT INTO groups (id, owner_id, workspace_id, name, free_speech, proactive_mode, scheduler_enabled, conversation_kind, direct_agent_id, title_source, status, created_at, updated_at) VALUES (?, ?, ?, ?, 1, 0, 0, 'direct', ?, 'automatic', 'active', ?, ?)")
        .bind(&id).bind(&owner_id).bind(&agent.workspace_id).bind(&title).bind(&agent.id).bind(&now).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to create direct chat"))?;
    sqlx::query("INSERT INTO group_members (group_id, user_id, role, status, joined_at) VALUES (?, ?, 'owner', 'active', ?)")
        .bind(&id).bind(&owner_id).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to create direct chat membership"))?;
    sqlx::query("INSERT INTO group_agents (group_id, agent_id, response_mode, context_scope_json, status, joined_at, updated_at) VALUES (?, ?, 'default', '{\"share_group_workspace\":true}', 'active', ?, ?)")
        .bind(&id).bind(&agent.id).bind(&now).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to bind direct chat agent"))?;
    sqlx::query("INSERT INTO threads (id, group_id, agent_id, status, next_seq, created_at, updated_at) VALUES (?, ?, NULL, 'active', 1, ?, ?)")
        .bind(Uuid::new_v4().to_string()).bind(&id).bind(&now).bind(&now).execute(&mut *tx).await
        .map_err(|_| ApiError::internal("failed to create direct chat thread"))?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit direct chat"))?;
    Ok((
        StatusCode::CREATED,
        Json(fetch(state.db.pool(), &id, &owner_id).await?),
    ))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DirectChatResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let rows = sqlx::query_as::<_, DirectChatResponse>("SELECT g.id, g.name AS title, g.title_source, g.direct_agent_id AS agent_id, a.name AS agent_name, a.status AS agent_status, g.workspace_id, g.status, g.created_at, g.updated_at FROM groups g LEFT JOIN agents a ON a.id = g.direct_agent_id WHERE g.owner_id = ? AND g.status = 'active' AND g.conversation_kind = 'direct' ORDER BY g.updated_at DESC, g.id DESC")
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
