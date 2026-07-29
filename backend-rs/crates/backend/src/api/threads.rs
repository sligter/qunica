//! Thread endpoints.
//!
//! Resume preflight runs before the SSE body is opened so missing, forbidden
//! and non-paused threads return normal JSON errors rather than in-stream
//! failures.

use std::convert::Infallible;

use ag_swarmer_domain::events::StreamEvent;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    Json,
};
use futures_util::{stream::BoxStream, StreamExt};
use serde::Serialize;
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    api::{
        auth::current_user_id,
        error::ApiError,
        sse_replay::{fetch_replay_events_for_thread, last_event_id, parse_replay_cursor},
        AppState,
    },
    runtime::{
        group::{run_thread_resume, ResumeRequest},
        RuntimeServices,
    },
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
    goal: Option<String>,
    status: String,
    priority: i64,
    started_at: Option<String>,
    completed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ThreadAccessRow {
    id: String,
    group_id: String,
    agent_id: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
    group_owner_id: Option<String>,
    group_status: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct InterruptedMessageRow {
    id: String,
    sender_type: String,
    sender_id: Option<String>,
    message_type: String,
    content: Option<String>,
}

struct ResumeTarget {
    group_id: String,
    thread_id: String,
    agent_id: String,
    message_id: String,
    existing_content: String,
}

impl From<ThreadAccessRow> for ThreadResponse {
    fn from(row: ThreadAccessRow) -> Self {
        Self {
            id: row.id,
            group_id: row.group_id,
            agent_id: row.agent_id,
            created_by: None,
            thread_type: None,
            title: None,
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

pub async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
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

    let target = resolve_resume_target(state.db.pool(), &thread_id, &owner_id).await?;
    claim_resume_thread(
        state.db.pool(),
        state.write_lock.as_ref(),
        &target.thread_id,
    )
    .await?;

    let (tx, rx) = mpsc::channel::<StreamEvent<Value>>(CHANNEL_CAPACITY);
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone())
        .with_active_turn_registry(state.active_turns.clone());
    let request = ResumeRequest {
        group_id: target.group_id,
        thread_id: target.thread_id,
        agent_id: target.agent_id,
        message_id: target.message_id,
        existing_content: target.existing_content,
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
    })
}

async fn claim_resume_thread(
    pool: &sqlx::SqlitePool,
    write_lock: &tokio::sync::Mutex<()>,
    thread_id: &str,
) -> Result<(), ApiError> {
    let _guard = write_lock.lock().await;
    let now = now_rfc3339();
    let result = sqlx::query(
        "UPDATE threads \
         SET status = 'running', updated_at = ? \
         WHERE id = ? AND status = 'paused'",
    )
    .bind(&now)
    .bind(thread_id)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    if result.rows_affected() == 0 {
        return Err(ApiError::conflict("thread is not paused"));
    }
    Ok(())
}

async fn fetch_owned_thread(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
    owner_id: &str,
) -> Result<ThreadAccessRow, ApiError> {
    let row: Option<ThreadAccessRow> = sqlx::query_as(
        "SELECT t.id, t.group_id, t.agent_id, t.status, t.created_at, t.updated_at, \
                g.owner_id AS group_owner_id, g.status AS group_status \
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

async fn latest_interrupted_message(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
) -> Result<InterruptedMessageRow, ApiError> {
    sqlx::query_as(
        "SELECT id, sender_type, sender_id, message_type, content \
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

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
