use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::SqlitePool;
use std::{collections::BTreeSet, path::PathBuf};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const GROUP_COLUMNS: &str = "id, owner_id, workspace_id, name, description, announcement, \
     free_speech, proactive_mode, proactive_max_rounds, proactive_reply_multiplier, \
     allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
     muted_agent_ids_json, admin_agent_ids_json, muted_member_ids_json, status, \
     created_at, updated_at";

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
