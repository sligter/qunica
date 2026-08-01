//! The approval queue for changes the Assistant proposed.
//!
//! A staged row does nothing on its own. [`approve`] is the only code path that
//! applies one, it is reachable only through an authenticated request the user
//! initiated, and it applies the change by calling the very same `*_inner` core
//! the UI route calls — so a staged change cannot bypass a validation the
//! normal path performs.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::tools::app_control::{write::Action, AppControlContext, TargetKind};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AppActionResponse {
    id: String,
    conversation_id: Option<String>,
    target_kind: String,
    action: String,
    target_id: Option<String>,
    summary: String,
    status: String,
    result_json: Option<String>,
    created_at: String,
    resolved_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ActionRow {
    id: String,
    target_kind: String,
    action: String,
    target_id: Option<String>,
    payload_json: String,
    status: String,
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AppActionResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let rows = sqlx::query_as::<_, AppActionResponse>(
        "SELECT id, conversation_id, target_kind, action, target_id, summary, status, \
                result_json, created_at, resolved_at \
         FROM app_actions WHERE owner_id = ? ORDER BY created_at DESC, id DESC LIMIT 200",
    )
    .bind(&owner_id)
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    Ok(Json(rows))
}

pub async fn reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
) -> Result<Json<AppActionResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let row = load_pending(&state, &action_id, &owner_id).await?;
    resolve(&state, &row.id, "rejected", None).await?;
    Ok(Json(fetch(&state, &action_id, &owner_id).await?))
}

/// Apply a staged change. The only path that mutates.
pub async fn approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
) -> Result<Json<AppActionResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    // Re-read and re-check the status under the write lock. Two approvals
    // racing would otherwise both see `pending` and apply the change twice.
    let guard = state.write_lock.lock().await;
    let row = load_pending(&state, &action_id, &owner_id).await?;
    drop(guard);

    let kind = TargetKind::parse(&row.target_kind)
        .ok_or_else(|| ApiError::internal("staged action has an unknown kind"))?;
    let action = parse_action(&row.action)?;
    let payload: Value = serde_json::from_str(&row.payload_json)
        .map_err(|_| ApiError::internal("staged action has an unreadable payload"))?;

    match apply(
        &state,
        &owner_id,
        kind,
        action,
        row.target_id.as_deref(),
        payload,
    )
    .await
    {
        Ok(applied) => {
            resolve(&state, &row.id, "applied", Some(&applied)).await?;
            Ok(Json(fetch(&state, &action_id, &owner_id).await?))
        }
        Err(error) => {
            // The world can move between proposal and approval — a workspace
            // deleted, a name taken. Record why so the history page can say,
            // and surface the original status rather than a generic 500.
            let status = error.status_code();
            let reason = json!({ "error": error.message_text() });
            resolve(&state, &row.id, "failed", Some(&reason)).await?;
            Err(ApiError::new(
                if status.is_server_error() {
                    status
                } else {
                    StatusCode::UNPROCESSABLE_ENTITY
                },
                "app_action_failed",
                error.message_text(),
            ))
        }
    }
}

/// Dispatch an approved action to the handler core that owns it.
///
/// Every arm calls the same `*_inner` the HTTP route calls. That is what makes
/// "the Assistant cannot do anything the user could not do themselves" true in
/// code rather than by convention.
async fn apply(
    state: &AppState,
    owner_id: &str,
    kind: TargetKind,
    action: Action,
    target_id: Option<&str>,
    payload: Value,
) -> Result<Value, ApiError> {
    match (kind, action) {
        (TargetKind::Agent, Action::Create) => {
            let body = decode(payload)?;
            let created = crate::api::agents::create_inner(state, owner_id, body).await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::Agent, Action::Update) => {
            let body = decode(payload)?;
            let updated =
                crate::api::agents::update_inner(state, owner_id, require(target_id)?, body)
                    .await?;
            Ok(serde_json::to_value(updated).unwrap_or_default())
        }
        (TargetKind::Skill, Action::Create) => {
            let body = decode(payload)?;
            let created = crate::api::skills::create_inner(state, owner_id, body).await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::Skill, Action::Update) => {
            let body = decode(payload)?;
            let updated =
                crate::api::skills::update_inner(state, owner_id, require(target_id)?, body)
                    .await?;
            Ok(serde_json::to_value(updated).unwrap_or_default())
        }
        (TargetKind::Workspace, Action::Create) => {
            let body = decode(payload)?;
            let created = crate::api::workspaces::create_inner(state, owner_id, body).await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::Workspace, Action::Update) => {
            let body = decode(payload)?;
            let updated =
                crate::api::workspaces::update_inner(state, owner_id, require(target_id)?, body)
                    .await?;
            Ok(serde_json::to_value(updated).unwrap_or_default())
        }
        (TargetKind::Group, Action::Create) => {
            let body = decode(payload)?;
            let created = crate::api::groups::create_inner(state, owner_id, body).await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::Group, Action::Update) => {
            let body = decode(payload)?;
            let updated =
                crate::api::groups::update_inner(state, owner_id, require(target_id)?, body)
                    .await?;
            Ok(serde_json::to_value(updated).unwrap_or_default())
        }
        (TargetKind::Mcp, Action::Create) => {
            let body = decode(payload)?;
            let created = crate::api::mcp_servers::create_inner(state, owner_id, body).await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::Mcp, Action::Update) => {
            let body = decode(payload)?;
            let updated =
                crate::api::mcp_servers::update_inner(state, owner_id, require(target_id)?, body)
                    .await?;
            Ok(serde_json::to_value(updated).unwrap_or_default())
        }
        // Unreachable through `AppPropose`, whose allowlist refuses these. A
        // row could only carry one if the database were edited directly.
        _ => Err(ApiError::invalid_input(
            "that combination cannot be applied",
        )),
    }
}

/// Check a proposal without applying it.
///
/// Called by `AppPropose` so a payload that cannot apply is refused while it is
/// still the model's problem. It deserializes into the same request type the
/// core takes and re-runs ownership checks; it deliberately does not write.
pub(crate) async fn validate_proposal(
    ctx: &AppControlContext,
    kind: TargetKind,
    action: Action,
    target_id: Option<&str>,
    payload: &Value,
) -> Result<(), crate::tools::ToolError> {
    use crate::tools::ToolError;

    let invalid = |error: ApiError| ToolError::invalid(error.message_text());

    // Deserializing into the real request type catches a missing required
    // field, a wrong type, and an unknown shape in one step.
    match kind {
        TargetKind::Agent if action == Action::Create => {
            decode::<crate::api::agents::CreateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Agent => {
            decode::<crate::api::agents::UpdateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Skill if action == Action::Create => {
            decode::<crate::api::skills::CreateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Skill => {
            decode::<crate::api::skills::UpdateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Workspace if action == Action::Create => {
            decode::<crate::api::workspaces::CreateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Workspace => {
            decode::<crate::api::workspaces::UpdateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Group if action == Action::Create => {
            decode::<crate::api::groups::CreateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Group => {
            decode::<crate::api::groups::UpdateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Mcp => {
            decode::<crate::api::mcp_servers::CreateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Provider | TargetKind::Chat => {
            return Err(ToolError::invalid("that kind cannot be proposed"))
        }
    }

    // Field-level rules the request types cannot express.
    check_payload_rules(kind, payload).map_err(invalid)?;

    // An update must name a row this owner actually holds. Without this the
    // refusal would only come at approval time, by which point the user has
    // already been shown a card naming someone else's resource.
    if let Some(target_id) = target_id {
        ensure_owned(ctx, kind, target_id).await?;
    }
    Ok(())
}

/// Validation that lives in the handler bodies rather than their types.
///
/// Mirrors the checks `*_inner` performs before touching the database. Anything
/// missed here is still caught at approval; the cost is only that the user sees
/// a card that then fails.
fn check_payload_rules(kind: TargetKind, payload: &Value) -> Result<(), ApiError> {
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    if let Some(name) = object.get("name").and_then(Value::as_str) {
        let length = name.trim().chars().count();
        if !(1..=100).contains(&length) {
            return Err(ApiError::invalid_input(
                "name must be between 1 and 100 characters",
            ));
        }
    }

    for (key, field) in [
        ("workspace_id", "workspace_id"),
        ("llm_provider_id", "llm_provider_id"),
    ] {
        if let Some(raw) = object.get(key).and_then(Value::as_str) {
            uuid::Uuid::parse_str(raw.trim())
                .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))?;
        }
    }

    if kind == TargetKind::Group {
        if let Some(mode) = object.get("communication_mode").and_then(Value::as_str) {
            if !matches!(mode.trim(), "mesh" | "star" | "hierarchical" | "ring") {
                return Err(ApiError::invalid_input(
                    "communication_mode must be one of mesh, star, hierarchical, or ring",
                ));
            }
        }
        if let Some(policy) = object.get("agent_mention_policy").and_then(Value::as_str) {
            if !matches!(policy.trim(), "display_only" | "bounded_schedule") {
                return Err(ApiError::invalid_input(
                    "agent_mention_policy must be display_only or bounded_schedule",
                ));
            }
        }
    }

    if kind == TargetKind::Mcp {
        let transport = object
            .get("transport")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !matches!(transport, "http" | "sse") {
            return Err(ApiError::invalid_input(
                "transport must be http or sse for a staged MCP server",
            ));
        }
    }

    if kind == TargetKind::Workspace {
        if let Some(backend) = object.get("backend_type").and_then(Value::as_str) {
            if !matches!(backend.trim(), "local" | "cloud_sandbox") {
                return Err(ApiError::invalid_input(
                    "backend_type must be 'local' or 'cloud_sandbox'",
                ));
            }
        }
    }

    Ok(())
}

/// Confirm the caller owns the row an update names, and that it is writable.
async fn ensure_owned(
    ctx: &AppControlContext,
    kind: TargetKind,
    target_id: &str,
) -> Result<(), crate::tools::ToolError> {
    use crate::tools::ToolError;

    let (table, extra) = match kind {
        // `is_system = 0` keeps the Assistant from proposing edits to itself.
        TargetKind::Agent => ("agents", "AND is_system = 0"),
        TargetKind::Skill => ("skills", ""),
        TargetKind::Workspace => ("workspaces", ""),
        TargetKind::Group => ("groups", "AND conversation_kind = 'group'"),
        TargetKind::Mcp => ("mcp_servers", ""),
        TargetKind::Provider | TargetKind::Chat => {
            return Err(ToolError::invalid("that kind cannot be proposed"))
        }
    };
    let sql = format!(
        "SELECT COUNT(*) FROM {table} \
         WHERE id = ? AND owner_id = ? AND status = 'active' {extra}"
    );
    let found: i64 = sqlx::query_scalar(&sql)
        .bind(target_id)
        .bind(ctx.owner_id())
        .fetch_one(ctx.pool())
        .await
        .map_err(|_| ToolError::invalid("could not check that target"))?;
    if found == 0 {
        // Same message whether it is missing or someone else's: distinguishing
        // them would confirm the id belongs to another account.
        return Err(ToolError::invalid(format!(
            "no {} with that id",
            kind.as_str()
        )));
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, ApiError> {
    serde_json::from_value(payload)
        .map_err(|error| ApiError::invalid_input(format!("invalid payload: {error}")))
}

fn require(target_id: Option<&str>) -> Result<&str, ApiError> {
    target_id.ok_or_else(|| ApiError::invalid_input("target_id is required"))
}

fn parse_action(raw: &str) -> Result<Action, ApiError> {
    match raw {
        "create" => Ok(Action::Create),
        "update" => Ok(Action::Update),
        _ => Err(ApiError::internal("staged action has an unknown action")),
    }
}

async fn load_pending(
    state: &AppState,
    action_id: &str,
    owner_id: &str,
) -> Result<ActionRow, ApiError> {
    let row = sqlx::query_as::<_, ActionRow>(
        "SELECT id, target_kind, action, target_id, payload_json, status \
         FROM app_actions WHERE id = ? AND owner_id = ?",
    )
    .bind(action_id)
    .bind(owner_id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?
    // Another owner's action is reported as missing, not forbidden.
    .ok_or_else(|| ApiError::not_found("action not found"))?;

    if row.status != "pending" {
        return Err(ApiError::conflict(format!(
            "this action was already {}",
            row.status
        )));
    }
    Ok(row)
}

async fn resolve(
    state: &AppState,
    action_id: &str,
    status: &str,
    result: Option<&Value>,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE app_actions SET status = ?, result_json = ?, resolved_at = ? \
         WHERE id = ? AND status = 'pending'",
    )
    .bind(status)
    .bind(result.map(|value| value.to_string()))
    .bind(now_rfc3339())
    .bind(action_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to record the action outcome"))?;
    Ok(())
}

async fn fetch(
    state: &AppState,
    action_id: &str,
    owner_id: &str,
) -> Result<AppActionResponse, ApiError> {
    sqlx::query_as::<_, AppActionResponse>(
        "SELECT id, conversation_id, target_kind, action, target_id, summary, status, \
                result_json, created_at, resolved_at \
         FROM app_actions WHERE id = ? AND owner_id = ?",
    )
    .bind(action_id)
    .bind(owner_id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("action not found"))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
