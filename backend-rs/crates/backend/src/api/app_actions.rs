//! The approval queue for changes the Assistant proposed.
//!
//! A staged row does nothing on its own. [`approve`] is the only code path that
//! applies one, it is reachable only through an authenticated request authorized
//! by the user, and it applies the change by calling the very same `*_inner`
//! core the UI route calls — so a staged change cannot bypass a validation the
//! normal path performs.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::tools::app_control::{write::Action, AppControlContext, TargetKind};

const DEFAULT_APP_ACTION_LIMIT: usize = 50;
const MAX_APP_ACTION_LIMIT: usize = 100;
const MAX_APP_ACTION_SKIP: usize = 10_000;

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

#[derive(Debug, Deserialize)]
pub struct AppActionListQuery {
    #[serde(default = "default_app_action_limit")]
    limit: usize,
    #[serde(default)]
    skip: usize,
    /// Free text matched against the summary, the target kind, and the action.
    #[serde(default)]
    q: Option<String>,
    /// One status to keep, or absent for every status.
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AppActionListResponse {
    items: Vec<AppActionResponse>,
    has_more: bool,
    /// Rows matching the filters, ignoring the page window.
    ///
    /// A history worth searching is one worth knowing the size of: without it
    /// the pager can only say "there is more", never how much more.
    total: i64,
}

const MAX_APP_ACTION_SEARCH_CHARS: usize = 200;

/// Statuses a row can hold, and therefore the only values worth filtering on.
const APP_ACTION_STATUSES: [&str; 6] = [
    "pending", "approved", "applied", "rejected", "failed", "expired",
];

fn default_app_action_limit() -> usize {
    DEFAULT_APP_ACTION_LIMIT
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

#[derive(Deserialize)]
struct ChatCreatePayload {
    agent_id: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessagePayload {
    message: String,
}

#[derive(Deserialize)]
struct GroupNoteCreatePayload {
    group_id: String,
    #[serde(flatten)]
    note: crate::api::groups::GroupNoteCreateRequest,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum GroupMembershipChange {
    AddAgent { agent_id: String },
    RemoveAgent { agent_id: String },
    AddUser { email: String },
    RemoveUser { email: String },
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AppActionListQuery>,
) -> Result<Json<AppActionListResponse>, ApiError> {
    if !(1..=MAX_APP_ACTION_LIMIT).contains(&query.limit) || query.skip > MAX_APP_ACTION_SKIP {
        return Err(ApiError::invalid_input(
            "app action pagination is out of bounds",
        ));
    }
    let search = normalize_search(query.q.as_deref())?;
    let status = normalize_status_filter(query.status.as_deref())?;
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    // `LIKE` with an escaped pattern rather than FTS: the history is a page of
    // recent proposals, not a corpus, and an index would cost more to keep than
    // the scan it saves.
    let predicate = "WHERE owner_id = ? \
         AND (? IS NULL OR status = ?) \
         AND (? IS NULL OR summary LIKE ? ESCAPE '\\' OR target_kind LIKE ? ESCAPE '\\' \
              OR action LIKE ? ESCAPE '\\')";
    let pattern = search.map(|value| format!("%{value}%"));

    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM app_actions {predicate}"))
        .bind(&owner_id)
        .bind(&status)
        .bind(&status)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_one(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    let sql = format!(
        "SELECT id, conversation_id, target_kind, action, target_id, summary, status, \
                result_json, created_at, resolved_at \
         FROM app_actions {predicate} ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
    );
    let mut items = sqlx::query_as::<_, AppActionResponse>(&sql)
        .bind(&owner_id)
        .bind(&status)
        .bind(&status)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind((query.limit + 1) as i64)
        .bind(query.skip as i64)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    let has_more = items.len() > query.limit;
    items.truncate(query.limit);
    Ok(Json(AppActionListResponse {
        items,
        has_more,
        total,
    }))
}

/// Escape the wildcards a person types so a summary containing `%` is findable
/// rather than a pattern that matches everything.
fn normalize_search(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > MAX_APP_ACTION_SEARCH_CHARS {
        return Err(ApiError::invalid_input(format!(
            "search must be at most {MAX_APP_ACTION_SEARCH_CHARS} characters"
        )));
    }
    Ok(Some(
        value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_"),
    ))
}

fn normalize_status_filter(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if !APP_ACTION_STATUSES.contains(&value) {
        return Err(ApiError::invalid_input("unknown app action status"));
    }
    Ok(Some(value.to_string()))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let row = load_action(&state, &action_id, &owner_id).await?;
    if !matches!(
        row.status.as_str(),
        "applied" | "rejected" | "failed" | "expired"
    ) {
        return Err(ApiError::conflict("unfinished actions cannot be deleted"));
    }

    sqlx::query(
        "DELETE FROM app_actions WHERE id = ? AND owner_id = ? \
         AND status IN ('applied', 'rejected', 'failed', 'expired')",
    )
    .bind(&action_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete assistant action"))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    sqlx::query(
        "DELETE FROM app_actions WHERE owner_id = ? \
         AND status IN ('applied', 'rejected', 'failed', 'expired')",
    )
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to clear assistant action history"))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
) -> Result<Json<AppActionResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let row = load_action(&state, &action_id, &owner_id).await?;
    if row.status == "rejected" {
        return Ok(Json(fetch(&state, &action_id, &owner_id).await?));
    }
    if row.status != "pending" {
        return Err(already_resolved(&row.status));
    }
    if !transition_pending(&state, &row.id, "rejected", None).await? {
        let current = fetch(&state, &action_id, &owner_id).await?;
        if current.status == "rejected" {
            return Ok(Json(current));
        }
        return Err(already_resolved(&current.status));
    }
    Ok(Json(fetch(&state, &action_id, &owner_id).await?))
}

/// Apply a staged change. The only path that mutates.
pub async fn approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
) -> Result<Json<AppActionResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let row = load_action(&state, &action_id, &owner_id).await?;
    if matches!(row.status.as_str(), "approved" | "applied") {
        return Ok(Json(fetch(&state, &action_id, &owner_id).await?));
    }
    if row.status != "pending" {
        return Err(already_resolved(&row.status));
    }

    let kind = TargetKind::parse(&row.target_kind)
        .ok_or_else(|| ApiError::internal("staged action has an unknown kind"))?;
    let action = parse_action(&row.action)?;
    let payload: Value = serde_json::from_str(&row.payload_json)
        .map_err(|_| ApiError::internal("staged action has an unreadable payload"))?;

    // Claim in SQLite before applying. The handler cores use the shared write
    // lock themselves, so holding it across `apply` would deadlock; a guarded
    // status transition gives us the same single-winner guarantee instead.
    if !transition_pending(&state, &row.id, "approved", None).await? {
        let current = fetch(&state, &action_id, &owner_id).await?;
        if matches!(current.status.as_str(), "approved" | "applied") {
            return Ok(Json(current));
        }
        return Err(already_resolved(&current.status));
    }

    if runs_in_background(kind, action, &payload) {
        let task_state = state.clone();
        let task_owner_id = owner_id.clone();
        let task_action_id = row.id.clone();
        let task_target_id = row.target_id.clone();
        tokio::spawn(async move {
            if let Err(error) = apply_approved(
                &task_state,
                &task_owner_id,
                &task_action_id,
                kind,
                action,
                task_target_id.as_deref(),
                payload,
            )
            .await
            {
                tracing::warn!(
                    action_id = %task_action_id,
                    error = %error.message_text(),
                    "background assistant action failed"
                );
            }
        });
        return Ok(Json(fetch(&state, &action_id, &owner_id).await?));
    }

    apply_approved(
        &state,
        &owner_id,
        &row.id,
        kind,
        action,
        row.target_id.as_deref(),
        payload,
    )
    .await?;
    Ok(Json(fetch(&state, &action_id, &owner_id).await?))
}

fn runs_in_background(kind: TargetKind, action: Action, payload: &Value) -> bool {
    match (kind, action) {
        (TargetKind::Chat, Action::Update) => true,
        (TargetKind::Chat, Action::Create)
        | (TargetKind::Group, Action::Create)
        | (TargetKind::Group, Action::Update) => payload.get("message").is_some(),
        _ => false,
    }
}

async fn apply_approved(
    state: &AppState,
    owner_id: &str,
    action_id: &str,
    kind: TargetKind,
    action: Action,
    target_id: Option<&str>,
    payload: Value,
) -> Result<(), ApiError> {
    match apply(state, owner_id, kind, action, target_id, payload).await {
        Ok(applied) => {
            finish_approval(state, action_id, "applied", Some(&applied)).await?;
            Ok(())
        }
        Err(error) => {
            // The world can move between proposal and approval — a workspace
            // deleted, a name taken. Record why so the history page can say,
            // and surface the original status rather than a generic 500.
            let status = error.status_code();
            let message = error.message_text();
            let reason = json!({ "error": message });
            finish_approval(state, action_id, "failed", Some(&reason)).await?;
            Err(ApiError::new(
                if status.is_server_error() {
                    status
                } else {
                    StatusCode::UNPROCESSABLE_ENTITY
                },
                "app_action_failed",
                message,
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
            let (payload, message, membership) = split_group_payload(payload)?;
            if membership.is_some() {
                return Err(ApiError::invalid_input(
                    "use initial_agents when creating a group",
                ));
            }
            let body = decode(payload)?;
            let created = crate::api::groups::create_inner(state, owner_id, body).await?;
            let Some(message) = message else {
                return Ok(serde_json::to_value(created).unwrap_or_default());
            };
            let group_id = created.id().to_string();
            let sent =
                crate::api::messages::send_group_inner(state, owner_id, &group_id, message).await?;
            Ok(json!({ "group": created, "message": sent }))
        }
        (TargetKind::Group, Action::Update) => {
            let group_id = require(target_id)?;
            let (payload, message, membership) = split_group_payload(payload)?;
            if let Some(change) = membership {
                if message.is_some() || !payload.as_object().is_some_and(|fields| fields.is_empty())
                {
                    return Err(ApiError::invalid_input(
                        "propose membership changes separately from settings and messages",
                    ));
                }
                return apply_group_membership(state, owner_id, group_id, change).await;
            }
            let Some(message) = message else {
                let body = decode(payload)?;
                let updated =
                    crate::api::groups::update_inner(state, owner_id, group_id, body).await?;
                return Ok(serde_json::to_value(updated).unwrap_or_default());
            };
            let updated = if payload.as_object().is_some_and(|fields| fields.is_empty()) {
                None
            } else {
                let body = decode(payload)?;
                Some(crate::api::groups::update_inner(state, owner_id, group_id, body).await?)
            };
            let sent =
                crate::api::messages::send_group_inner(state, owner_id, group_id, message).await?;
            Ok(json!({ "group": updated, "group_id": group_id, "message": sent }))
        }
        (TargetKind::GroupTemplate, Action::Create) => {
            let body = decode(payload)?;
            let created =
                crate::api::groups::create_group_template_inner(state, owner_id, body).await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::GroupNote, Action::Create) => {
            let body: GroupNoteCreatePayload = decode(payload)?;
            let created = crate::api::groups::create_group_note_inner(
                state,
                owner_id,
                &body.group_id,
                body.note,
            )
            .await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        (TargetKind::GroupNote, Action::Update) => {
            let body = decode(payload)?;
            let updated = crate::api::groups::update_group_note_by_id_inner(
                state,
                owner_id,
                require(target_id)?,
                body,
            )
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
        (TargetKind::Chat, Action::Create) => {
            let body: ChatCreatePayload = decode(payload)?;
            let created =
                crate::api::direct_chats::create_inner(state, owner_id, &body.agent_id).await?;
            let chat_id = created.id().to_string();
            let sent = match body.message {
                Some(message) => Some(
                    crate::api::messages::send_direct_inner(state, owner_id, &chat_id, message)
                        .await?,
                ),
                None => None,
            };
            Ok(json!({ "chat": created, "message": sent }))
        }
        (TargetKind::Chat, Action::Update) => {
            let body: ChatMessagePayload = decode(payload)?;
            let chat_id = require(target_id)?;
            let sent =
                crate::api::messages::send_direct_inner(state, owner_id, chat_id, body.message)
                    .await?;
            Ok(json!({ "chat_id": chat_id, "message": sent }))
        }
        // Unreachable through `AppPropose`, whose allowlist refuses these. A
        // row could only carry one if the database were edited directly.
        _ => Err(ApiError::invalid_input(
            "that combination cannot be applied",
        )),
    }
}

async fn apply_group_membership(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    change: GroupMembershipChange,
) -> Result<Value, ApiError> {
    match change {
        GroupMembershipChange::AddAgent { agent_id } => {
            let member = crate::api::groups::add_group_agent_inner(
                state,
                owner_id,
                group_id,
                crate::api::groups::GroupAgentAddRequest::with_default_workspace(agent_id),
            )
            .await?;
            Ok(json!({ "operation": "add_agent", "member": member }))
        }
        GroupMembershipChange::RemoveAgent { agent_id } => {
            crate::api::groups::remove_group_agent_inner(state, owner_id, group_id, &agent_id)
                .await?;
            Ok(json!({ "operation": "remove_agent", "agent_id": agent_id }))
        }
        GroupMembershipChange::AddUser { email } => {
            let (user_id, email) = find_user_by_email(state.db.pool(), &email).await?;
            let member =
                crate::api::groups::add_group_member_inner(state, owner_id, group_id, &user_id)
                    .await?;
            Ok(json!({ "operation": "add_user", "email": email, "member": member }))
        }
        GroupMembershipChange::RemoveUser { email } => {
            let (user_id, email) = find_user_by_email(state.db.pool(), &email).await?;
            crate::api::groups::remove_group_member_inner(state, owner_id, group_id, &user_id)
                .await?;
            Ok(json!({ "operation": "remove_user", "email": email, "user_id": user_id }))
        }
    }
}

async fn find_user_by_email(pool: &SqlitePool, email: &str) -> Result<(String, String), ApiError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(ApiError::invalid_input("email must not be empty"));
    }
    sqlx::query_as("SELECT id, email FROM users WHERE LOWER(email) = LOWER(?) LIMIT 1")
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))?
        .ok_or_else(|| ApiError::not_found("user not found"))
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
            let (payload, _, membership) = split_group_payload(payload.clone()).map_err(invalid)?;
            if membership.is_some() {
                return Err(ToolError::invalid(
                    "use initial_agents when creating a group",
                ));
            }
            decode::<crate::api::groups::CreateRequest>(payload.clone()).map_err(invalid)?;
            if let Some(template_id) = payload.get("template_id").and_then(Value::as_str) {
                ensure_owned(ctx, TargetKind::GroupTemplate, template_id).await?;
            }
        }
        TargetKind::Group => {
            let (payload, message, membership) =
                split_group_payload(payload.clone()).map_err(invalid)?;
            if let Some(change) = membership {
                if message.is_some() || !payload.as_object().is_some_and(|fields| fields.is_empty())
                {
                    return Err(ToolError::invalid(
                        "propose membership changes separately from settings and messages",
                    ));
                }
                let group_id = target_id
                    .ok_or_else(|| ToolError::invalid("target_id is required to update"))?;
                validate_group_membership(ctx, group_id, &change).await?;
            }
            decode::<crate::api::groups::UpdateRequest>(payload).map_err(invalid)?;
        }
        TargetKind::GroupTemplate => {
            decode::<crate::api::groups::GroupTemplateCreateRequest>(payload.clone())
                .map_err(invalid)?;
            let group_id = payload
                .get("group_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::invalid("group_id is required"))?;
            ensure_owned(ctx, TargetKind::Group, group_id).await?;
        }
        TargetKind::GroupNote if action == Action::Create => {
            let body = decode::<GroupNoteCreatePayload>(payload.clone()).map_err(invalid)?;
            ensure_owned(ctx, TargetKind::Group, &body.group_id).await?;
        }
        TargetKind::GroupNote => {
            decode::<crate::api::groups::GroupNoteUpdateRequest>(payload.clone())
                .map_err(invalid)?;
        }
        TargetKind::Mcp => {
            decode::<crate::api::mcp_servers::CreateRequest>(payload.clone()).map_err(invalid)?;
        }
        TargetKind::Chat if action == Action::Create => {
            let body = decode::<ChatCreatePayload>(payload.clone()).map_err(invalid)?;
            if let Some(message) = body.message.as_deref() {
                validate_chat_message(message).map_err(invalid)?;
            }
            ensure_owned(ctx, TargetKind::Agent, &body.agent_id).await?;
        }
        TargetKind::Chat => {
            let body = decode::<ChatMessagePayload>(payload.clone()).map_err(invalid)?;
            validate_chat_message(&body.message).map_err(invalid)?;
        }
        TargetKind::Provider => return Err(ToolError::invalid("that kind cannot be proposed")),
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
        ("provider_id", "llm_provider_id"),
        ("agent_id", "agent_id"),
        ("group_id", "group_id"),
        ("template_id", "template_id"),
    ] {
        if let Some(raw) = object.get(key).and_then(Value::as_str) {
            uuid::Uuid::parse_str(raw.trim())
                .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))?;
        }
    }

    if kind == TargetKind::Group {
        if let Some(mode) = object.get("scheduler_mode").and_then(Value::as_str) {
            if !matches!(mode.trim(), "bounded" | "automatic") {
                return Err(ApiError::invalid_input(
                    "scheduler_mode must be bounded or automatic",
                ));
            }
        }
        if let Some(mode) = object.get("communication_mode").and_then(Value::as_str) {
            if !matches!(mode.trim(), "mesh" | "star" | "hierarchical" | "ring") {
                return Err(ApiError::invalid_input(
                    "communication_mode must be one of mesh, star, hierarchical, or ring",
                ));
            }
        }
        if let Some(field) = [
            "agent_mention_policy",
            "allow_agent_free_mention",
            "agent_free_mention_max_dispatches",
        ]
        .into_iter()
        .find(|field| object.contains_key(*field))
        {
            return Err(ApiError::invalid_input(format!(
                "{field} has been removed; agent @mentions are display-only, use AgentAsTool for delegation"
            )));
        }
    }

    if kind == TargetKind::GroupNote {
        if let Some(title) = object.get("title").and_then(Value::as_str) {
            crate::api::groups::validate_note_title(title)?;
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

async fn validate_group_membership(
    ctx: &AppControlContext,
    group_id: &str,
    change: &GroupMembershipChange,
) -> Result<(), crate::tools::ToolError> {
    use crate::tools::ToolError;

    ensure_owned(ctx, TargetKind::Group, group_id).await?;
    match change {
        GroupMembershipChange::AddAgent { agent_id } => {
            ensure_owned(ctx, TargetKind::Agent, agent_id).await?;
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM group_agents \
                 WHERE group_id = ? AND agent_id = ? AND status = 'active'",
            )
            .bind(group_id)
            .bind(agent_id)
            .fetch_one(ctx.pool())
            .await
            .map_err(|_| ToolError::invalid("could not inspect group membership"))?;
            if exists > 0 {
                return Err(ToolError::invalid("agent is already in the group"));
            }
        }
        GroupMembershipChange::RemoveAgent { agent_id } => {
            ensure_owned(ctx, TargetKind::Agent, agent_id).await?;
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM group_agents \
                 WHERE group_id = ? AND agent_id = ? AND status = 'active'",
            )
            .bind(group_id)
            .bind(agent_id)
            .fetch_one(ctx.pool())
            .await
            .map_err(|_| ToolError::invalid("could not inspect group membership"))?;
            if exists == 0 {
                return Err(ToolError::invalid("agent is not in the group"));
            }
        }
        GroupMembershipChange::AddUser { email } => {
            let (user_id, _) = find_user_by_email(ctx.pool(), email)
                .await
                .map_err(|error| ToolError::invalid(error.message_text()))?;
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM group_members \
                 WHERE group_id = ? AND user_id = ? AND status = 'active'",
            )
            .bind(group_id)
            .bind(user_id)
            .fetch_one(ctx.pool())
            .await
            .map_err(|_| ToolError::invalid("could not inspect group membership"))?;
            if exists > 0 {
                return Err(ToolError::invalid("user is already in the group"));
            }
        }
        GroupMembershipChange::RemoveUser { email } => {
            let (user_id, _) = find_user_by_email(ctx.pool(), email)
                .await
                .map_err(|error| ToolError::invalid(error.message_text()))?;
            let role: Option<String> = sqlx::query_scalar(
                "SELECT role FROM group_members \
                 WHERE group_id = ? AND user_id = ? AND status = 'active'",
            )
            .bind(group_id)
            .bind(user_id)
            .fetch_optional(ctx.pool())
            .await
            .map_err(|_| ToolError::invalid("could not inspect group membership"))?;
            match role.as_deref() {
                None => return Err(ToolError::invalid("user is not in the group")),
                Some("owner") => return Err(ToolError::invalid("group owner cannot be removed")),
                Some(_) => {}
            }
        }
    }
    Ok(())
}

fn validate_chat_message(message: &str) -> Result<(), ApiError> {
    if message.trim().is_empty() {
        Err(ApiError::invalid_input("message must not be empty"))
    } else {
        Ok(())
    }
}

fn split_group_payload(
    mut payload: Value,
) -> Result<(Value, Option<String>, Option<GroupMembershipChange>), ApiError> {
    let fields = payload
        .as_object_mut()
        .ok_or_else(|| ApiError::invalid_input("payload must be an object"))?;
    let message = match fields.remove("message") {
        None => None,
        Some(Value::String(message)) => {
            validate_chat_message(&message)?;
            Some(message)
        }
        Some(_) => return Err(ApiError::invalid_input("message must be a string")),
    };
    let membership = fields.remove("membership").map(decode).transpose()?;
    Ok((payload, message, membership))
}

/// Confirm the caller owns the row an update names, and that it is writable.
async fn ensure_owned(
    ctx: &AppControlContext,
    kind: TargetKind,
    target_id: &str,
) -> Result<(), crate::tools::ToolError> {
    use crate::tools::ToolError;

    if kind == TargetKind::GroupTemplate {
        let found: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM group_templates WHERE id = ? AND owner_id = ?",
        )
        .bind(target_id)
        .bind(ctx.owner_id())
        .fetch_one(ctx.pool())
        .await
        .map_err(|_| ToolError::invalid("could not check that target"))?;
        return ensure_target_found(found, kind);
    }
    if kind == TargetKind::GroupNote {
        let found: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM group_notes n JOIN groups g ON g.id = n.group_id \
             WHERE n.id = ? AND n.status = 'active' AND g.owner_id = ? \
               AND g.status = 'active' AND g.conversation_kind = 'group'",
        )
        .bind(target_id)
        .bind(ctx.owner_id())
        .fetch_one(ctx.pool())
        .await
        .map_err(|_| ToolError::invalid("could not check that target"))?;
        return ensure_target_found(found, kind);
    }

    let (table, extra) = match kind {
        // `is_system = 0` keeps the Assistant from proposing edits to itself.
        TargetKind::Agent => ("agents", "AND is_system = 0"),
        TargetKind::Skill => ("skills", ""),
        TargetKind::Workspace => ("workspaces", ""),
        TargetKind::Group => ("groups", "AND conversation_kind = 'group'"),
        TargetKind::Chat => (
            "groups",
            "AND conversation_kind = 'direct' AND direct_agent_id IN \
             (SELECT id FROM agents WHERE is_system = 0 AND status = 'active')",
        ),
        TargetKind::Mcp => ("mcp_servers", ""),
        TargetKind::GroupTemplate | TargetKind::GroupNote => unreachable!(),
        TargetKind::Provider => return Err(ToolError::invalid("that kind cannot be proposed")),
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
    ensure_target_found(found, kind)
}

fn ensure_target_found(found: i64, kind: TargetKind) -> Result<(), crate::tools::ToolError> {
    if found > 0 {
        return Ok(());
    }
    // Same message whether it is missing or someone else's: distinguishing
    // them would confirm the id belongs to another account.
    Err(crate::tools::ToolError::invalid(format!(
        "no {} with that id",
        kind.as_str()
    )))
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

async fn load_action(
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

    Ok(row)
}

fn already_resolved(status: &str) -> ApiError {
    ApiError::conflict(format!("this action was already {status}"))
}

async fn transition_pending(
    state: &AppState,
    action_id: &str,
    status: &str,
    result: Option<&Value>,
) -> Result<bool, ApiError> {
    let changed = sqlx::query(
        "UPDATE app_actions SET status = ?, result_json = ?, resolved_at = ? \
         WHERE id = ? AND status = 'pending'",
    )
    .bind(status)
    .bind(result.map(|value| value.to_string()))
    .bind(now_rfc3339())
    .bind(action_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to record the action outcome"))?
    .rows_affected();
    Ok(changed == 1)
}

async fn finish_approval(
    state: &AppState,
    action_id: &str,
    status: &str,
    result: Option<&Value>,
) -> Result<(), ApiError> {
    let changed = sqlx::query(
        "UPDATE app_actions SET status = ?, result_json = ?, resolved_at = ? \
         WHERE id = ? AND status = 'approved'",
    )
    .bind(status)
    .bind(result.map(|value| value.to_string()))
    .bind(now_rfc3339())
    .bind(action_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to record the action outcome"))?
    .rows_affected();
    if changed != 1 {
        return Err(ApiError::internal("approval state changed while applying"));
    }
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
