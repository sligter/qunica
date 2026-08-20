//! Thread endpoints.
//!
//! Resume preflight runs before the SSE body is opened so missing, forbidden
//! and non-paused threads return normal JSON errors rather than in-stream
//! failures.

use std::{convert::Infallible, path::Path as FsPath};

use ag_swarmer_domain::events::StreamEvent;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    Json,
};
use futures_util::{stream::BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    api::{
        auth::current_user_id,
        conversations::{ensure_active_owned_conversation, ConversationKind},
        error::ApiError,
        sse_replay::{fetch_replay_events_for_thread, last_event_id, parse_replay_cursor},
        workspace_files::{self, ConversationRoot, ConversationScope},
        AppState,
    },
    git::{self as workspace_git, TaskWorktree},
    runtime::{
        group::{run_thread_resume, ResumeRequest},
        RuntimeServices,
    },
    tools::ApprovalDecision,
};

const CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Serialize)]
pub struct ThreadResponse {
    id: String,
    group_id: String,
    agent_id: Option<String>,
    created_by: Option<String>,
    thread_type: Option<String>,
    title: Option<String>,
    git_branch: Option<String>,
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

struct ResumeTarget {
    group_id: String,
    thread_id: String,
    agent_id: String,
    message_id: String,
    existing_content: String,
    content_json: Option<String>,
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
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    Ok(Json(ThreadResponse::from(thread)))
}

pub async fn list_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<ThreadResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    ensure_active_owned_conversation(
        state.db.pool(),
        &group_id,
        &owner_id,
        ConversationKind::Group,
    )
    .await?;

    let rows: Vec<ThreadAccessRow> = sqlx::query_as(
        "SELECT t.id, t.group_id, t.agent_id, t.title, t.git_branch, t.status, t.created_at, t.updated_at, \
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
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
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
    let provisioned = if let Some(requested_branch) = requested_branch {
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

        let workspace = workspace_files::load_owned_local_workspace(
            state.db.pool(),
            ConversationRoot::conversation(ConversationScope::Groups, &group_id, &owner_id),
        )
        .await?;
        let worktree_path = state.task_worktree_root.join(&group_id).join(&id);
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
    .bind(provisioned.as_ref().map(|(_, _, binding)| &binding.branch))
    .bind(
        provisioned
            .as_ref()
            .map(|(_, path, _)| path.to_string_lossy().into_owned()),
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

pub async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
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
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
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
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
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
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
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
    let mut tx = state
        .db
        .pool()
        .begin()
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
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;
    state.active_turns.cancel_thread(&thread.id).await;
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
}

pub async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    body: Option<Json<ResumeBody>>,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let thread_id = validate_uuid(&thread_id, "thread id")?;
    let thread = fetch_owned_thread(state.db.pool(), &thread_id, &owner_id).await?;

    if let Some(cursor) = last_event_id(&headers)? {
        let cursor = parse_replay_cursor(&cursor)?;
        let events =
            fetch_replay_events_for_thread(state.db.pool(), &thread.id, &thread.group_id, &cursor)
                .await?;
        let body = futures_util::stream::iter(events.into_iter().map(event_to_sse)).boxed();
        return Ok(Sse::new(body));
    }

    let approval = body.and_then(|Json(body)| body.approval);
    if let Some(decision) = approval.as_ref() {
        if decision.tool_call_id.trim().is_empty() {
            return Err(ApiError::invalid_input(
                "approval.tool_call_id must not be empty",
            ));
        }
    }

    let target = resolve_resume_target(state.db.pool(), &thread_id, &owner_id).await?;
    let (tx, rx) = mpsc::channel::<StreamEvent<Value>>(CHANNEL_CAPACITY);
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone())
        .with_active_turn_registry(state.active_turns.clone());
    let request = ResumeRequest {
        group_id: target.group_id,
        thread_id: target.thread_id,
        agent_id: target.agent_id,
        message_id: target.message_id,
        existing_content: target.existing_content,
        content_json: target.content_json,
        approval,
    };
    tokio::spawn(async move {
        run_thread_resume(services, request, tx).await;
    });

    let body = futures_util::stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        Some((event_to_sse(event), rx))
    })
    .boxed();

    Ok(Sse::new(body))
}

async fn resolve_resume_target(
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
        thread_id: thread.id,
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
        "SELECT t.id, t.group_id, t.agent_id, t.title, t.git_branch, t.status, t.created_at, t.updated_at, \
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

fn event_to_sse(event: StreamEvent<Value>) -> Result<Event, Infallible> {
    let data = serde_json::to_string(&event).unwrap_or_default();
    Ok(Event::default().id(event.event_id.clone()).data(data))
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
