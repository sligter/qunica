//! Thread endpoints.
//!
//! Resume preflight runs before the SSE body is opened so missing, forbidden
//! and non-paused threads return normal JSON errors rather than in-stream
//! failures.

use std::path::Path as FsPath;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::{
    api::{
        auth::current_user_id,
        conversations::{ensure_active_owned_conversation, ConversationKind},
        error::ApiError,
        sse_replay::{
            last_event_id, parse_replay_cursor, replay_thread_stream, sse_response, SseResponse,
        },
        workspace_files::{self, ConversationRoot, ConversationScope},
        AppState,
    },
    git::{self as workspace_git, TaskWorktree},
    tools::ApprovalDecision,
};

#[derive(Debug, Serialize)]
pub struct ThreadResponse {
    id: String,
    group_id: String,
    agent_id: Option<String>,
    created_by: Option<String>,
    thread_type: Option<String>,
    title: Option<String>,
    git_branch: Option<String>,
    worktree_path: Option<String>,
    goal: Option<String>,
    status: String,
    priority: i64,
    started_at: Option<String>,
    completed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskThreadRequest {
    title: String,
    git_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenameTaskThreadRequest {
    title: String,
}

#[derive(Debug, Serialize)]
pub struct ClearTaskMessagesResponse {
    cleared_count: u64,
}

#[derive(Debug, sqlx::FromRow)]
struct ThreadAccessRow {
    id: String,
    group_id: String,
    agent_id: Option<String>,
    title: Option<String>,
    git_branch: Option<String>,
    worktree_path: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
    group_owner_id: Option<String>,
    group_status: Option<String>,
    conversation_kind: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct InterruptedMessageRow {
    id: String,
    sender_type: String,
    sender_id: Option<String>,
    message_type: String,
    content: Option<String>,
    content_json: Option<String>,
}

pub(crate) struct ResumeTarget {
    pub(crate) group_id: String,
    pub(crate) agent_id: String,
    pub(crate) message_id: String,
    pub(crate) existing_content: String,
    pub(crate) content_json: Option<String>,
}

impl From<ThreadAccessRow> for ThreadResponse {
    fn from(row: ThreadAccessRow) -> Self {
        let thread_type = match (row.agent_id.is_none(), row.conversation_kind.as_deref()) {
            (true, Some("group")) => Some("task_thread".to_string()),
            (true, Some("direct")) => Some("chat_thread".to_string()),
            _ => None,
        };
        Self {
            id: row.id,
            group_id: row.group_id,
            agent_id: row.agent_id,
            created_by: None,
            thread_type,
            title: row.title,
            git_branch: row.git_branch,
            worktree_path: row.worktree_path,
            goal: None,
            status: row.status,
            priority: 0,
            started_at: None,
            completed_at: None,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    Ok(Json(ThreadResponse::from(thread)))
}

pub async fn list_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<ThreadResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let group_id = validate_uuid(&group_id, "group id")?;
    ensure_active_owned_conversation(
        state.db.pool(),
        &group_id,
        &owner_id,
        ConversationKind::Group,
    )
    .await?;

    let rows: Vec<ThreadAccessRow> = sqlx::query_as(
        "SELECT t.id, t.group_id, t.agent_id, t.title, t.git_branch, t.worktree_path, t.status, t.created_at, t.updated_at, \
                g.owner_id AS group_owner_id, g.status AS group_status, \
                g.conversation_kind AS conversation_kind \
         FROM threads t \
         JOIN groups g ON g.id = t.group_id \
         WHERE t.group_id = ? AND t.agent_id IS NULL AND t.status != 'cleared' \
         ORDER BY CASE WHEN t.status = 'archived' THEN 1 ELSE 0 END, \
                  t.updated_at DESC, t.created_at DESC, t.id DESC",
    )
    .bind(&group_id)
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(ThreadResponse::from).collect()))
}

pub async fn create_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<CreateTaskThreadRequest>,
) -> Result<(StatusCode, Json<ThreadResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let title = validate_title(&body.title)?;
    let requested_branch = body
        .git_branch
        .as_deref()
        .map(str::trim)
        .filter(|branch| !branch.is_empty());
    ensure_active_owned_conversation(
        state.db.pool(),
        &group_id,
        &owner_id,
        ConversationKind::Group,
    )
    .await?;

    let id = Uuid::new_v4().to_string();
    let remote: bool = sqlx::query_scalar(
        "SELECT execution_environment_id IS NOT NULL FROM groups WHERE id=? AND owner_id=?",
    )
    .bind(&group_id)
    .bind(&owner_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    if let Some(branch) = requested_branch {
        if remote && !valid_computer_branch(branch) {
            return Err(ApiError::invalid_input("invalid task branch name"));
        }
        let already_bound: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM threads \
             WHERE group_id = ? AND agent_id IS NULL AND git_branch = ?)",
        )
        .bind(&group_id)
        .bind(requested_branch)
        .fetch_one(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;
        if already_bound {
            return Err(ApiError::conflict(
                "branch is already bound to another task",
            ));
        }
    }
    // Note: a computer task carries a branch, never a host mount. Provisioning
    // waits until Runtime owns the turn reservation; see docs/guide/computers.md.
    let remote_branch = requested_branch.filter(|_| remote);
    let provisioned = if let Some(requested_branch) = requested_branch.filter(|_| !remote) {
        let workspace = workspace_files::load_owned_local_workspace(
            state.db.pool(),
            ConversationRoot::conversation(ConversationScope::Groups, &group_id, &owner_id),
        )
        .await?;
        let worktree_path = workspace.root.join(".worktrees").join(&group_id).join(&id);
        let binding =
            workspace_git::create_task_worktree(&workspace.root, &worktree_path, requested_branch)
                .await
                .map_err(|error| ApiError::invalid_input(error.to_string()))?;
        let canonical_path = match tokio::fs::canonicalize(&worktree_path).await {
            Ok(path) => path,
            Err(_) => {
                rollback_task_worktree(&workspace.root, &worktree_path, &binding).await;
                return Err(ApiError::internal("failed to resolve task worktree path"));
            }
        };
        Some((workspace.root, canonical_path, binding))
    } else {
        None
    };
    let now = now_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO threads \
         (id, group_id, agent_id, title, git_branch, worktree_path, status, next_seq, created_at, updated_at) \
         VALUES (?, ?, NULL, ?, ?, ?, 'active', 1, ?, ?)",
    )
    .bind(&id)
    .bind(&group_id)
    .bind(title)
    .bind(provisioned.as_ref().map(|(_, _, binding)| binding.branch.as_str()).or(remote_branch))
    .bind(
        provisioned
            .as_ref()
            .map(|(_, path, _)| path.to_string_lossy().into_owned())
            .or_else(|| remote_branch.map(|_| format!("computer:.worktrees/{group_id}/{id}"))),
    )
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await;
    if inserted.is_err() {
        if let Some((root, path, binding)) = &provisioned {
            rollback_task_worktree(root, path, binding).await;
        }
        return Err(ApiError::internal("failed to create task"));
    }

    let thread = fetch_owned_thread(state.db.pool(), &id, &owner_id).await?;
    Ok((StatusCode::CREATED, Json(ThreadResponse::from(thread))))
}

pub async fn rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(body): Json<RenameTaskThreadRequest>,
) -> Result<Json<ThreadResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    ensure_task_thread(&thread)?;
    let title = validate_title(&body.title)?;

    // Note: A title is metadata, so active and archived tasks can be renamed
    // without changing their execution state, messages, or worktree binding.
    let updated = sqlx::query(
        "UPDATE threads SET title = ?, updated_at = ? WHERE id = ? AND status != 'cleared'",
    )
    .bind(title)
    .bind(now_rfc3339())
    .bind(&thread_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to rename task"))?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::not_found("task not found"));
    }

    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    Ok(Json(ThreadResponse::from(thread)))
}

pub async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    ensure_task_thread(&thread)?;
    if thread.status == "archived" {
        return Ok(Json(ThreadResponse::from(thread)));
    }
    if thread.status == "cleared" {
        return Err(ApiError::conflict("task has been cleared"));
    }

    let updated = sqlx::query(
        "UPDATE threads SET status = 'archived', updated_at = ? \
         WHERE id = ? AND status NOT IN ('running', 'archived', 'cleared') \
           AND NOT EXISTS ( \
             SELECT 1 FROM group_turns \
             WHERE thread_id = ? AND status IN ('pending', 'running', 'waiting_for_user') \
           )",
    )
    .bind(now_rfc3339())
    .bind(&thread_id)
    .bind(&thread_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to archive task"))?;
    if updated.rows_affected() == 0 {
        let current = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
        if current.status == "archived" {
            return Ok(Json(ThreadResponse::from(current)));
        }
        return Err(ApiError::conflict("task is still running"));
    }

    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    Ok(Json(ThreadResponse::from(thread)))
}

/// Restores an archived task so it accepts messages again.
///
/// Only the archived → active transition is a real change; every other live
/// status is returned untouched so a double click is harmless.
pub async fn unarchive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    ensure_task_thread(&thread)?;
    if thread.status == "cleared" {
        return Err(ApiError::not_found("task not found"));
    }
    if thread.status != "archived" {
        return Ok(Json(ThreadResponse::from(thread)));
    }

    sqlx::query(
        "UPDATE threads SET status = 'active', updated_at = ? \
         WHERE id = ? AND status = 'archived'",
    )
    .bind(now_rfc3339())
    .bind(&thread_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to restore task"))?;

    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    Ok(Json(ThreadResponse::from(thread)))
}

/// Clears one task's conversation without removing the task or touching its siblings.
pub async fn clear_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<Json<ClearTaskMessagesResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    ensure_task_thread(&thread)?;
    if thread.status == "cleared" {
        return Err(ApiError::not_found("task not found"));
    }

    let _guard = state.write_lock.lock().await;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM group_turns \
         WHERE thread_id = ? AND status IN ('pending', 'running', 'waiting_for_user'))",
    )
    .bind(&thread_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    if active {
        return Err(ApiError::conflict("task is still running"));
    }

    let cleared_count = sqlx::query(
        "UPDATE messages SET status = 'cleared' \
         WHERE thread_id = ? AND group_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(&thread_id)
    .bind(&thread.group_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to clear task messages"))?
    .rows_affected();

    Ok(Json(ClearTaskMessagesResponse { cleared_count }))
}

/// Deletes an archived task.
///
/// Archiving is the gate: a task has to be archived first, which is what rules
/// out deleting one that is still running. The rows are soft-deleted (`cleared`)
/// the same way message clearing works, and the git bindings are released so the
/// branch can back a new task.
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    ensure_task_thread(&thread)?;
    if thread.status == "cleared" {
        return Err(ApiError::not_found("task not found"));
    }
    if thread.status != "archived" {
        return Err(ApiError::conflict("task must be archived before deletion"));
    }

    let worktree_path: Option<String> =
        sqlx::query_scalar("SELECT worktree_path FROM threads WHERE id = ?")
            .bind(&thread_id)
            .fetch_one(state.db.pool())
            .await
            .map_err(|_| ApiError::internal("database error"))?;

    let _guard = state.write_lock.lock().await;
    let mut tx = crate::db::begin_write(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("failed to start task delete transaction"))?;

    sqlx::query(
        "UPDATE messages SET status = 'cleared' \
         WHERE thread_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(&thread_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to delete task messages"))?;

    let deleted = sqlx::query(
        "UPDATE threads \
         SET status = 'cleared', git_branch = NULL, worktree_path = NULL, updated_at = ? \
         WHERE id = ? AND status = 'archived'",
    )
    .bind(now_rfc3339())
    .bind(&thread_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to delete task"))?
    .rows_affected();
    if deleted == 0 {
        return Err(ApiError::conflict("task is no longer archived"));
    }

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit task delete"))?;
    drop(_guard);

    if let Some(worktree_path) = worktree_path {
        remove_thread_worktree(&state, &thread.group_id, &owner_id, &worktree_path).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    let active:Option<String>=sqlx::query_scalar("SELECT id FROM group_turns WHERE thread_id=? AND status IN ('pending','running','waiting_for_user') ORDER BY created_at DESC LIMIT 1")
        .bind(&thread.id).fetch_optional(state.db.pool()).await.map_err(|_|ApiError::internal("database error"))?;
    if let Some(turn) = active {
        super::commands::accept(
            &state,
            &owner_id,
            super::commands::Command {
                request_id: headers
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
                name: "turn.cancel".into(),
                target: serde_json::json!({"turn_id":turn}),
                payload: serde_json::json!({}),
            },
        )
        .await?;
    } else {
        state.active_turns.cancel_thread(&thread.id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Body of a resume request.
///
/// Empty for an ordinary continuation. A thread paused on a tool call carries
/// the user's answer here, and the runtime replays that exact call rather than
/// re-prompting the model — which is what makes the approval a control over the
/// command the user was shown, not over whatever the model proposes next.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ResumeBody {
    approval: Option<ApprovalDecision>,
    request_id: Option<String>,
    expected_version: Option<i64>,
    action_hash: Option<String>,
}

pub async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    body: Option<Json<ResumeBody>>,
) -> Result<SseResponse, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;

    if let Some(cursor) = last_event_id(&headers)? {
        let cursor = parse_replay_cursor(&cursor)?;
        let body =
            replay_thread_stream(state.db.pool().clone(), thread.id, thread.group_id, cursor)
                .await?;
        return Ok(sse_response(body));
    }

    let body = body.map(|Json(body)| body).unwrap_or_default();
    let approval = body.approval;
    if let Some(decision) = approval.as_ref() {
        if decision.tool_call_id.trim().is_empty() {
            return Err(ApiError::invalid_input(
                "approval.tool_call_id must not be empty",
            ));
        }
    }

    let target = resolve_resume_target(state.db.pool(), &thread_id, &owner_id).await?;
    crate::runtime::approval::ensure_pending(
        state.db.pool(),
        &thread_id,
        target.content_json.as_deref(),
    )
    .await
    .map_err(|_| ApiError::conflict("invalid approval checkpoint"))?;
    let mut payload = serde_json::to_value(&approval).unwrap_or(Value::Null);
    let name = if approval.is_some() {
        "approval.resolve"
    } else {
        "thread.resume"
    };
    if let Some(decision) = approval.as_ref() {
        let row:Option<(i64,String)>=sqlx::query_as("SELECT version,action_hash FROM pending_approvals WHERE thread_id=? AND tool_call_id=? AND status='pending'")
            .bind(&thread_id).bind(&decision.tool_call_id).fetch_optional(state.db.pool()).await.map_err(|_|ApiError::internal("database error"))?;
        let (version, hash) =
            row.ok_or_else(|| ApiError::conflict("no matching pending approval"))?;
        payload["expected_version"] = serde_json::json!(body.expected_version.unwrap_or(version));
        payload["action_hash"] = serde_json::json!(body.action_hash.unwrap_or(hash));
    }
    let response = super::commands::accept(
        &state,
        &owner_id,
        super::commands::Command {
            request_id: body
                .request_id
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            name: name.into(),
            target: serde_json::json!({"thread_id":thread_id}),
            payload,
        },
    )
    .await?;
    let stream = Uuid::parse_str(response["stream_id"].as_str().unwrap()).unwrap();
    Ok(sse_response(super::sse_replay::follow_new_stream(
        state.db.pool().clone(),
        thread.group_id,
        stream,
    )))
}

pub(crate) async fn resolve_resume_target(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
    owner_id: &str,
) -> Result<ResumeTarget, ApiError> {
    let thread = fetch_owned_thread(pool, thread_id, owner_id).await?;
    if thread.status != "paused" {
        return Err(ApiError::conflict("thread is not paused"));
    }

    let interrupted = latest_interrupted_message(pool, thread_id).await?;
    if interrupted.sender_type != "agent" || interrupted.message_type != "text" {
        return Err(ApiError::conflict(
            "thread has no interrupted agent text message to resume",
        ));
    }
    let agent_id = interrupted.sender_id.ok_or_else(|| {
        ApiError::conflict("thread has no interrupted agent text message to resume")
    })?;

    ensure_active_group_agent(pool, &thread.group_id, &agent_id).await?;

    Ok(ResumeTarget {
        group_id: thread.group_id,
        agent_id,
        message_id: interrupted.id,
        existing_content: interrupted.content.unwrap_or_default(),
        content_json: interrupted.content_json,
    })
}

async fn fetch_owned_thread(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
    owner_id: &str,
) -> Result<ThreadAccessRow, ApiError> {
    let row: Option<ThreadAccessRow> = sqlx::query_as(
        "SELECT t.id, t.group_id, t.agent_id, t.title, t.git_branch, t.worktree_path, t.status, t.created_at, t.updated_at, \
                g.owner_id AS group_owner_id, g.status AS group_status, \
                g.conversation_kind AS conversation_kind \
         FROM threads t \
         LEFT JOIN groups g ON g.id = t.group_id \
         WHERE t.id = ?",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    let row = row.ok_or_else(|| ApiError::not_found("thread not found"))?;
    match (&row.group_owner_id, &row.group_status) {
        (Some(_), Some(status)) if status != "active" => {
            return Err(ApiError::not_found("group not found"));
        }
        (None, _) | (_, None) => return Err(ApiError::not_found("group not found")),
        _ => {}
    }
    if row.group_owner_id.as_deref() != Some(owner_id) {
        return Err(ApiError::permission_denied(
            "thread belongs to another user's group",
        ));
    }
    Ok(row)
}

async fn rollback_task_worktree(root: &FsPath, path: &FsPath, binding: &TaskWorktree) {
    if let Err(error) = workspace_git::remove_task_worktree(root, path).await {
        tracing::warn!(%error, path = %path.display(), "failed to roll back task worktree");
        return;
    }
    if binding.created_branch {
        if let Err(error) = workspace_git::delete_branch(root, &binding.branch, true).await {
            tracing::warn!(%error, branch = %binding.branch, "failed to roll back task branch");
        }
    }
}

/// Detaches a deleted task's worktree from the shared repository.
///
/// Best effort on purpose: the task row is already gone, so a missing workspace
/// or a git failure is logged rather than surfaced — it must not leave the user
/// with a task they cannot delete. The branch itself is kept, since it may still
/// carry the work the task produced.
async fn remove_thread_worktree(
    state: &AppState,
    group_id: &str,
    owner_id: &str,
    worktree_path: &str,
) {
    // Remote home survives task deletion. Never pass its logical path to host Git.
    if worktree_path.starts_with("computer:") {
        return;
    }
    let workspace = workspace_files::load_owned_local_workspace(
        state.db.pool(),
        ConversationRoot::conversation(ConversationScope::Groups, group_id, owner_id),
    )
    .await;
    let Ok(workspace) = workspace else {
        tracing::warn!(
            group_id,
            path = worktree_path,
            "skipped task worktree cleanup: workspace is unavailable"
        );
        return;
    };
    if let Err(error) =
        workspace_git::remove_task_worktree(&workspace.root, FsPath::new(worktree_path)).await
    {
        tracing::warn!(%error, path = worktree_path, "failed to remove deleted task worktree");
    }
}

fn valid_computer_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 240
        && !matches!(branch, "HEAD" | "@")
        && !branch.starts_with('-')
        && !branch.ends_with('.')
        && !["..", "@{", "//"].iter().any(|part| branch.contains(part))
        && !branch
            .chars()
            .any(|ch| ch.is_control() || " ~^:?*[\\".contains(ch))
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

fn ensure_task_thread(thread: &ThreadAccessRow) -> Result<(), ApiError> {
    if thread.agent_id.is_some() || thread.conversation_kind.as_deref() != Some("group") {
        return Err(ApiError::not_found("task not found"));
    }
    Ok(())
}

async fn latest_interrupted_message(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
) -> Result<InterruptedMessageRow, ApiError> {
    sqlx::query_as(
        "SELECT id, sender_type, sender_id, message_type, content, content_json \
         FROM messages \
         WHERE thread_id = ? AND status = 'interrupted' \
         ORDER BY seq DESC, id DESC \
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::conflict("thread has no interrupted message to resume"))
}

async fn ensure_active_group_agent(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    agent_id: &str,
) -> Result<(), ApiError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 \
         FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? \
           AND ga.agent_id = ? \
           AND ga.status = 'active' \
           AND a.status = 'active' \
         LIMIT 1",
    )
    .bind(group_id)
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    if found.is_none() {
        return Err(ApiError::not_found("agent not found in group"));
    }
    Ok(())
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
}

fn validate_title(raw: &str) -> Result<String, ApiError> {
    let title = raw.trim();
    if title.is_empty() || title.chars().count() > 80 {
        return Err(ApiError::invalid_input(
            "title must be between 1 and 80 characters",
        ));
    }
    Ok(title.to_string())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
