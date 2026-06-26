use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const GROUP_COLUMNS: &str = "id, owner_id, workspace_id, name, description, free_speech, \
     proactive_mode, proactive_reply_multiplier, allow_agent_free_mention, status, \
     created_at, updated_at";

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    workspace_id: String,
    #[serde(default)]
    free_speech: Option<bool>,
    #[serde(default)]
    proactive_mode: Option<bool>,
    #[serde(default)]
    proactive_reply_multiplier: Option<i64>,
    #[serde(default)]
    allow_agent_free_mention: Option<bool>,
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
    workspace_id: Option<Option<String>>,
    #[serde(default)]
    free_speech: Option<bool>,
    #[serde(default)]
    proactive_mode: Option<bool>,
    #[serde(default)]
    proactive_reply_multiplier: Option<i64>,
    #[serde(default)]
    allow_agent_free_mention: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct GroupResponse {
    id: String,
    name: String,
    description: Option<String>,
    workspace_id: Option<String>,
    free_speech: bool,
    proactive_mode: bool,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: bool,
    status: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupRow {
    id: String,
    owner_id: String,
    workspace_id: Option<String>,
    name: String,
    description: Option<String>,
    // SQLite stores booleans as integers; these are exposed as booleans below.
    free_speech: i64,
    proactive_mode: i64,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: i64,
    status: String,
    created_at: String,
    updated_at: String,
}

impl From<GroupRow> for GroupResponse {
    fn from(row: GroupRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            workspace_id: row.workspace_id,
            free_speech: row.free_speech != 0,
            proactive_mode: row.proactive_mode != 0,
            proactive_reply_multiplier: row.proactive_reply_multiplier,
            allow_agent_free_mention: row.allow_agent_free_mention != 0,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
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
    // API v2 simplification: a group must bind to an existing owned workspace.
    // Automatic group workspace creation is deferred to system settings.
    let workspace_id = validate_workspace(state.db.pool(), &body.workspace_id, &owner_id).await?;
    let description = normalize_description(body.description.as_deref());
    let free_speech = body.free_speech.unwrap_or(false);
    let proactive_mode = body.proactive_mode.unwrap_or(false);
    let multiplier = validate_multiplier(body.proactive_reply_multiplier)?;
    let allow_agent_free_mention = body.allow_agent_free_mention.unwrap_or(true);

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    sqlx::query(
        "INSERT INTO groups \
         (id, owner_id, workspace_id, name, description, free_speech, proactive_mode, \
          proactive_reply_multiplier, allow_agent_free_mention, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(free_speech as i64)
    .bind(proactive_mode as i64)
    .bind(multiplier)
    .bind(allow_agent_free_mention as i64)
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to create group"))?;

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
    let multiplier = match body.proactive_reply_multiplier {
        Some(value) => validate_multiplier(Some(value))?,
        None => existing.proactive_reply_multiplier,
    };
    let allow_agent_free_mention = body
        .allow_agent_free_mention
        .map(|b| b as i64)
        .unwrap_or(existing.allow_agent_free_mention);

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE groups SET \
         name = ?, description = ?, workspace_id = ?, free_speech = ?, proactive_mode = ?, \
         proactive_reply_multiplier = ?, allow_agent_free_mention = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&description)
    .bind(&workspace_id)
    .bind(free_speech)
    .bind(proactive_mode)
    .bind(multiplier)
    .bind(allow_agent_free_mention)
    .bind(&now)
    .bind(&group_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update group"))?;

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
    sqlx::query("UPDATE groups SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ?")
        .bind(&now)
        .bind(&group_id)
        .bind(&owner_id)
        .execute(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("failed to delete group"))?;

    Ok(StatusCode::NO_CONTENT)
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
        Some((owner, _)) if owner != owner_id => {
            Err(ApiError::permission_denied("workspace belongs to another user"))
        }
        Some((_, status)) if status != "active" => {
            Err(ApiError::invalid_input("workspace is not active"))
        }
        Some(_) => Ok(id),
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

fn validate_multiplier(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(1);
    if value < 1 {
        return Err(ApiError::invalid_input(
            "proactive_reply_multiplier must be >= 1",
        ));
    }
    Ok(value)
}

fn normalize_description(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
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

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}
