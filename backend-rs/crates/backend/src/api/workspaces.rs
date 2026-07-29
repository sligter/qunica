use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use std::path::PathBuf;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const BACKEND_LOCAL: &str = "local";
const BACKEND_CLOUD_SANDBOX: &str = "cloud_sandbox";

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    backend_type: Option<String>,
    local_path: Option<String>,
    #[serde(default)]
    auto_create: bool,
    config: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    backend_type: Option<String>,
    // Double `Option` distinguishes an omitted field (outer `None`) from an
    // explicit JSON `null` (inner `None`).
    #[serde(default, deserialize_with = "double_option")]
    local_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    config: Option<Option<Value>>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    id: String,
    name: String,
    backend_type: String,
    local_path: Option<String>,
    config: Option<Value>,
    status: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct WorkspaceRow {
    id: String,
    owner_id: String,
    name: String,
    backend_type: String,
    local_path: Option<String>,
    config_json: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
}

impl From<WorkspaceRow> for WorkspaceResponse {
    fn from(row: WorkspaceRow) -> Self {
        let config = row
            .config_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        Self {
            id: row.id,
            name: row.name,
            backend_type: row.backend_type,
            local_path: row.local_path,
            config,
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
) -> Result<(StatusCode, Json<WorkspaceResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let name = validate_name(&body.name)?;
    let backend_type = normalize_backend_type(body.backend_type.as_deref())?;
    let id = Uuid::new_v4().to_string();
    let local_path = if body.auto_create {
        if backend_type != BACKEND_LOCAL
            || body
                .local_path
                .as_deref()
                .is_some_and(|path| !path.trim().is_empty())
        {
            return Err(ApiError::invalid_input(
                "auto_create requires a local backend without local_path",
            ));
        }
        Some(create_local_workspace_dir(state.db.pool(), &owner_id, &id).await?)
    } else {
        resolve_local_path(&backend_type, body.local_path.as_deref())?
    };
    let config_json = to_config_json(body.config.as_ref());

    let now = now_rfc3339();

    sqlx::query(
        "INSERT INTO workspaces \
         (id, owner_id, name, backend_type, local_path, config_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&name)
    .bind(&backend_type)
    .bind(&local_path)
    .bind(&config_json)
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to create workspace"))?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("workspace vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

async fn create_local_workspace_dir(
    pool: &SqlitePool,
    owner_id: &str,
    workspace_id: &str,
) -> Result<String, ApiError> {
    let root = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT group_workspace_root FROM system_settings WHERE owner_id = ?",
    )
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .and_then(|(root,)| root)
    .filter(|root| !root.trim().is_empty())
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "workspace_root_required",
            "group_workspace_root is required",
        )
    })?;

    let short_id: String = workspace_id
        .chars()
        .filter(|ch| *ch != '-')
        .take(8)
        .collect();
    let path = PathBuf::from(root).join(format!("workspace-{short_id}"));
    std::fs::create_dir_all(&path)
        .map_err(|_| ApiError::internal("failed to create workspace directory"))?;
    std::fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| ApiError::internal("failed to resolve workspace directory"))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<WorkspaceResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let rows = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, owner_id, name, backend_type, local_path, config_json, status, created_at, updated_at \
         FROM workspaces \
         WHERE owner_id = ? AND status = 'active' \
         ORDER BY created_at DESC, id DESC",
    )
    .bind(&owner_id)
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(
        rows.into_iter().map(WorkspaceResponse::from).collect(),
    ))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let workspace_id = parse_id(&workspace_id)?;

    let row = load_owned(state.db.pool(), &workspace_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let workspace_id = parse_id(&workspace_id)?;

    let existing = load_owned(state.db.pool(), &workspace_id, &owner_id).await?;

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    let backend_type = match body.backend_type.as_deref() {
        Some(raw) => normalize_backend_type(Some(raw))?,
        None => existing.backend_type.clone(),
    };

    // Only revalidate/canonicalize the path when the client explicitly sends
    // one; otherwise the stored binding is preserved untouched.
    let local_path = match body.local_path {
        Some(ref provided) => resolve_local_path(&backend_type, provided.as_deref())?,
        None => {
            if backend_type == BACKEND_LOCAL && existing.local_path.is_none() {
                return Err(ApiError::invalid_input(
                    "local_path is required for local backend",
                ));
            }
            existing.local_path.clone()
        }
    };
    let config_json = match body.config {
        Some(ref provided) => to_config_json(provided.as_ref()),
        None => existing.config_json.clone(),
    };

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE workspaces \
         SET name = ?, backend_type = ?, local_path = ?, config_json = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&backend_type)
    .bind(&local_path)
    .bind(&config_json)
    .bind(&now)
    .bind(&workspace_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update workspace"))?;

    let row = fetch_row(state.db.pool(), &workspace_id)
        .await?
        .ok_or_else(|| ApiError::internal("workspace vanished after update"))?;
    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let workspace_id = parse_id(&workspace_id)?;

    // Confirms existence and ownership before mutating anything.
    load_owned(state.db.pool(), &workspace_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE workspaces SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&workspace_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete workspace"))?;

    // Unbind same-owner active agents/groups so they no longer point at a
    // workspace that no longer exists from their perspective.
    for table in ["agents", "groups"] {
        let sql = format!(
            "UPDATE {table} SET workspace_id = NULL, updated_at = ? \
             WHERE workspace_id = ? AND owner_id = ? AND status = 'active'"
        );
        sqlx::query(&sql)
            .bind(&now)
            .bind(&workspace_id)
            .bind(&owner_id)
            .execute(state.db.pool())
            .await
            .map_err(|_| ApiError::internal("failed to unbind workspace references"))?;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Fetch a workspace by id and enforce that the caller owns it.
///
/// Returns `404 not_found` when no such row exists, and `403 permission_denied`
/// when the row exists but belongs to another user.
async fn load_owned(
    pool: &SqlitePool,
    workspace_id: &str,
    owner_id: &str,
) -> Result<WorkspaceRow, ApiError> {
    let row = fetch_row(pool, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace not found"))?;
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "workspace belongs to another user",
        ));
    }
    Ok(row)
}

async fn fetch_row(
    pool: &SqlitePool,
    workspace_id: &str,
) -> Result<Option<WorkspaceRow>, ApiError> {
    sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, owner_id, name, backend_type, local_path, config_json, status, created_at, updated_at \
         FROM workspaces WHERE id = ?",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
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

fn normalize_backend_type(raw: Option<&str>) -> Result<String, ApiError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(BACKEND_LOCAL.to_string()),
        Some(BACKEND_LOCAL) => Ok(BACKEND_LOCAL.to_string()),
        Some(BACKEND_CLOUD_SANDBOX) => Ok(BACKEND_CLOUD_SANDBOX.to_string()),
        Some(_) => Err(ApiError::invalid_input(
            "backend_type must be 'local' or 'cloud_sandbox'",
        )),
    }
}

/// Resolve the stored `local_path` for a given backend.
///
/// `local` backends require an existing directory and store its canonical
/// absolute path. `cloud_sandbox` stores any non-empty value as-is and treats
/// empty/missing input as `NULL`.
fn resolve_local_path(backend_type: &str, raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let trimmed = raw.map(str::trim).filter(|p| !p.is_empty());
    if backend_type == BACKEND_LOCAL {
        let path = trimmed
            .ok_or_else(|| ApiError::invalid_input("local_path is required for local backend"))?;
        let canonical = std::fs::canonicalize(path)
            .map_err(|_| ApiError::invalid_input("local_path must be an existing directory"))?;
        if !canonical.is_dir() {
            return Err(ApiError::invalid_input(
                "local_path must be an existing directory",
            ));
        }
        Ok(Some(canonical.to_string_lossy().into_owned()))
    } else {
        Ok(trimmed.map(|p| p.to_string()))
    }
}

fn to_config_json(config: Option<&Value>) -> Option<String> {
    match config {
        Some(value) if !value.is_null() => serde_json::to_string(value).ok(),
        _ => None,
    }
}

fn parse_id(raw: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw)
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input("invalid workspace id"))
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
