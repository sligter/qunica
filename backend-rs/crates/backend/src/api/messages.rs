//! Group message endpoints.
//!
//! The streaming endpoint relays runtime [`StreamEvent`]s as Server-Sent
//! Events. The non-stream send endpoint runs the same runtime while draining
//! its bounded channel, then shapes a frontend-compatible response from the
//! durable runtime events and persisted message rows.

use std::convert::Infallible;

use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
use axum::{
    extract::{Path, Query, State},
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
        error::ApiError,
        sse_replay::{
            event_kind_from_wire, fetch_replay_events_for_group, last_event_id, parse_replay_cursor,
        },
        AppState,
    },
    runtime::{run_group_turn, RuntimeServices, TurnOutcome, TurnRequest},
};

/// Buffered events between the runtime task and the SSE response body. Bounded
/// so a slow/absent client applies backpressure (and so disconnects surface as
/// a failed send rather than unbounded growth).
const CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Deserialize)]
pub struct StreamRequest {
    content: String,
    #[serde(default)]
    thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    content: String,
    #[serde(default)]
    thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    limit: Option<String>,
    before: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    id: String,
    group_id: String,
    thread_id: Option<String>,
    sender_type: String,
    sender_id: Option<String>,
    message_type: String,
    content: Option<String>,
    status: String,
    refs: Option<Value>,
    context_usage: Option<Value>,
    reply_to_message_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ClearMessagesResponse {
    cleared_count: u64,
}

#[derive(Debug, Serialize)]
pub struct SilentAgentTurnResponse {
    agent_id: String,
    display_name: String,
}

#[derive(Debug, Serialize)]
pub struct MessageSendResponse {
    user_message: MessageResponse,
    agent_replies: Vec<MessageResponse>,
    dispatch_messages: Vec<MessageResponse>,
    warnings: Vec<String>,
    silent_turns: Vec<SilentAgentTurnResponse>,
    all_silent: bool,
    waiting_for_user: bool,
}

#[derive(Debug)]
struct DurableEventRef {
    event_id: String,
    kind: StreamEventKind,
}

#[derive(Debug)]
struct PersistedEventRow {
    kind: String,
    payload: Value,
}

#[derive(Debug, sqlx::FromRow)]
struct MessageRow {
    id: String,
    group_id: String,
    thread_id: String,
    sender_type: String,
    sender_id: Option<String>,
    message_type: String,
    content: Option<String>,
    content_json: Option<String>,
    status: String,
    created_at: String,
}

impl From<MessageRow> for MessageResponse {
    fn from(row: MessageRow) -> Self {
        let context_usage = context_usage_from_content_json(row.content_json.as_deref());
        Self {
            id: row.id,
            group_id: row.group_id,
            thread_id: Some(row.thread_id),
            sender_type: row.sender_type,
            sender_id: row.sender_id,
            message_type: row.message_type,
            content: row.content,
            status: row.status,
            refs: None,
            context_usage,
            reply_to_message_id: None,
            created_at: row.created_at,
        }
    }
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let limit = parse_limit(query.limit.as_deref())?;
    let before_id = query
        .before
        .as_deref()
        .map(|raw| validate_uuid(raw, "before message id"))
        .transpose()?;

    ensure_active_owned_group(state.db.pool(), &group_id, &owner_id).await?;

    let before_cursor = match before_id {
        Some(before_id) => {
            Some(fetch_visible_message_cursor(state.db.pool(), &group_id, &before_id).await?)
        }
        None => None,
    };

    let mut rows = fetch_message_page(state.db.pool(), &group_id, limit, before_cursor).await?;
    rows.reverse();
    Ok(Json(rows.into_iter().map(MessageResponse::from).collect()))
}

pub async fn clear(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<ClearMessagesResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    ensure_active_owned_group(state.db.pool(), &group_id, &owner_id).await?;

    let _guard = state.write_lock.lock().await;
    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start message clear transaction"))?;

    let cleared_count = sqlx::query(
        "UPDATE messages \
         SET status = 'cleared' \
         WHERE group_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to clear messages"))?
    .rows_affected();

    sqlx::query(
        "UPDATE threads \
         SET status = 'cleared', updated_at = ? \
         WHERE group_id = ? \
           AND agent_id IS NULL \
           AND status IN ('active', 'running', 'paused', 'completed', 'failed', 'created') \
           AND NOT EXISTS ( \
             SELECT 1 FROM messages \
             WHERE messages.thread_id = threads.id \
               AND messages.group_id = ? \
               AND messages.status IN ('visible', 'interrupted') \
           )",
    )
    .bind(&now)
    .bind(&group_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to clear message threads"))?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit message clear"))?;

    Ok(Json(ClearMessagesResponse { cleared_count }))
}

pub async fn send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<SendRequest>,
) -> Result<(StatusCode, Json<MessageSendResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(ApiError::invalid_input("content must not be empty"));
    }

    ensure_active_owned_group(state.db.pool(), &group_id, &owner_id).await?;

    let (tx, mut rx) = mpsc::channel::<StreamEvent<Value>>(CHANNEL_CAPACITY);
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone());
    let request = TurnRequest {
        group_id: group_id.clone(),
        owner_id,
        thread_id: body.thread_id,
        content,
    };
    let handle = tokio::spawn(async move { run_group_turn(services, request, tx).await });

    let mut durable_events = Vec::new();
    let mut warnings = Vec::new();
    while let Some(event) = rx.recv().await {
        if is_durable_response_event(&event.kind) {
            durable_events.push(DurableEventRef {
                event_id: event.event_id.clone(),
                kind: event.kind.clone(),
            });
        }
        if matches!(
            event.kind,
            StreamEventKind::Warning | StreamEventKind::Error
        ) {
            if let Some(message) = event_message(&event.payload) {
                warnings.push(message);
            }
        }
    }

    let outcome = handle
        .await
        .map_err(|_| ApiError::internal("message send task failed"))?;

    let mut user_message_id: Option<String> = None;
    let mut agent_reply_ids = Vec::new();
    let mut dispatch_message_ids = Vec::new();
    let mut silent_turns = Vec::new();
    let mut waiting_for_user = false;
    let mut saw_silence = false;

    for event_ref in durable_events {
        let persisted = fetch_persisted_event(state.db.pool(), &event_ref.event_id).await?;
        let kind = event_kind_from_wire(&persisted.kind)?;
        if kind != event_ref.kind {
            return Err(ApiError::internal("runtime event kind mismatch"));
        }
        match kind {
            StreamEventKind::UserMessage => {
                if let Some(id) = message_id_from_payload(&persisted.payload) {
                    user_message_id = Some(id.to_string());
                }
            }
            StreamEventKind::AgentMessage => {
                if let Some(id) = message_id_from_payload(&persisted.payload) {
                    if persisted
                        .payload
                        .get("dispatch")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        dispatch_message_ids.push(id.to_string());
                    } else {
                        agent_reply_ids.push(id.to_string());
                    }
                }
            }
            StreamEventKind::AgentSilent => {
                if let (Some(agent_id), Some(display_name)) = (
                    persisted.payload.get("agent_id").and_then(Value::as_str),
                    persisted
                        .payload
                        .get("display_name")
                        .and_then(Value::as_str),
                ) {
                    silent_turns.push(SilentAgentTurnResponse {
                        agent_id: agent_id.to_string(),
                        display_name: display_name.to_string(),
                    });
                }
            }
            StreamEventKind::WaitingForUser => {
                waiting_for_user = true;
            }
            StreamEventKind::Silence => {
                saw_silence = true;
            }
            _ => {}
        }
    }

    let Some(user_message_id) = user_message_id else {
        let message = warnings
            .first()
            .cloned()
            .unwrap_or_else(|| "message send failed".to_string());
        return Err(ApiError::invalid_input(message));
    };

    let user_message = fetch_message_response(state.db.pool(), &group_id, &user_message_id).await?;
    let mut agent_replies = Vec::new();
    for id in agent_reply_ids {
        agent_replies.push(fetch_message_response(state.db.pool(), &group_id, &id).await?);
    }
    let mut dispatch_messages = Vec::new();
    for id in dispatch_message_ids {
        dispatch_messages.push(fetch_message_response(state.db.pool(), &group_id, &id).await?);
    }

    let all_silent = saw_silence && !silent_turns.is_empty() && agent_replies.is_empty();
    if all_silent {
        warnings.push("No one replied".to_string());
    }
    if matches!(outcome, TurnOutcome::Cancelled) {
        warnings.push("Message send was cancelled".to_string());
    }

    Ok((
        StatusCode::CREATED,
        Json(MessageSendResponse {
            user_message,
            agent_replies,
            dispatch_messages,
            warnings,
            silent_turns,
            all_silent,
            waiting_for_user,
        }),
    ))
}

pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<StreamRequest>,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(ApiError::invalid_input("content must not be empty"));
    }

    ensure_active_owned_group(state.db.pool(), &group_id, &owner_id).await?;

    if let Some(cursor) = last_event_id(&headers)? {
        let cursor = parse_replay_cursor(&cursor)?;
        let events = fetch_replay_events_for_group(state.db.pool(), &group_id, &cursor).await?;
        let body = futures_util::stream::iter(events.into_iter().map(event_to_sse)).boxed();
        return Ok(Sse::new(body));
    }

    let (tx, rx) = mpsc::channel::<StreamEvent<Value>>(CHANNEL_CAPACITY);
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone());
    let request = TurnRequest {
        group_id,
        owner_id,
        thread_id: body.thread_id,
        content,
    };
    tokio::spawn(async move {
        run_group_turn(services, request, tx).await;
    });

    let body = futures_util::stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        Some((event_to_sse(event), rx))
    })
    .boxed();

    Ok(Sse::new(body))
}

fn event_to_sse(event: StreamEvent<Value>) -> Result<Event, Infallible> {
    let data = serde_json::to_string(&event).unwrap_or_default();
    Ok(Event::default().id(event.event_id.clone()).data(data))
}

async fn fetch_persisted_event(
    pool: &sqlx::SqlitePool,
    event_id: &str,
) -> Result<PersistedEventRow, ApiError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT kind, payload_json FROM stream_events WHERE event_id = ?")
            .bind(event_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;
    let (kind, payload_json) =
        row.ok_or_else(|| ApiError::internal("runtime event was not persisted"))?;
    let payload = serde_json::from_str(&payload_json)
        .map_err(|_| ApiError::internal("runtime event payload was invalid"))?;
    Ok(PersistedEventRow { kind, payload })
}

async fn fetch_message_response(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    message_id: &str,
) -> Result<MessageResponse, ApiError> {
    let row = sqlx::query_as::<_, MessageRow>(
        "SELECT id, group_id, thread_id, sender_type, sender_id, message_type, \
                content, content_json, status, created_at \
         FROM messages \
         WHERE id = ? AND group_id = ?",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::internal("runtime message row was not persisted"))?;
    Ok(MessageResponse::from(row))
}

async fn fetch_visible_message_cursor(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    message_id: &str,
) -> Result<(i64, String), ApiError> {
    sqlx::query_as::<_, (i64, String)>(
        "SELECT seq, id FROM messages \
         WHERE id = ? AND group_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("message not found"))
}

async fn fetch_message_page(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    limit: i64,
    before_cursor: Option<(i64, String)>,
) -> Result<Vec<MessageRow>, ApiError> {
    let rows = match before_cursor {
        Some((before_seq, before_id)) => {
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, group_id, thread_id, seq, sender_type, sender_id, message_type, \
                        content, content_json, status, created_at \
                 FROM messages \
                 WHERE group_id = ? \
                   AND status IN ('visible', 'interrupted') \
                   AND (seq < ? OR (seq = ? AND id < ?)) \
                 ORDER BY seq DESC, id DESC \
                 LIMIT ?",
            )
            .bind(group_id)
            .bind(before_seq)
            .bind(before_seq)
            .bind(before_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, group_id, thread_id, seq, sender_type, sender_id, message_type, \
                        content, content_json, status, created_at \
                 FROM messages \
                 WHERE group_id = ? AND status IN ('visible', 'interrupted') \
                 ORDER BY seq DESC, id DESC \
                 LIMIT ?",
            )
            .bind(group_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    };

    rows.map_err(|_| ApiError::internal("database error"))
}

/// Confirm the group exists, is active, and belongs to the caller.
async fn ensure_active_owned_group(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT owner_id, status FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::not_found("group not found")),
        Some((_, status)) if status == "deleted" => Err(ApiError::not_found("group not found")),
        Some((owner, _)) if owner != owner_id => {
            Err(ApiError::permission_denied("group belongs to another user"))
        }
        Some(_) => Ok(()),
    }
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
}

fn parse_limit(raw: Option<&str>) -> Result<i64, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(30);
    };
    let limit = raw
        .parse::<i64>()
        .map_err(|_| ApiError::invalid_input("limit must be an integer"))?;
    Ok(limit.clamp(1, 100))
}

fn context_usage_from_content_json(raw: Option<&str>) -> Option<Value> {
    let value: Value = serde_json::from_str(raw?).ok()?;
    match value.get("context_usage") {
        Some(Value::Object(_)) => value.get("context_usage").cloned(),
        _ => None,
    }
}

fn is_durable_response_event(kind: &StreamEventKind) -> bool {
    matches!(
        kind,
        StreamEventKind::UserMessage
            | StreamEventKind::AgentMessage
            | StreamEventKind::AgentSilent
            | StreamEventKind::WaitingForUser
            | StreamEventKind::Silence
    )
}

fn message_id_from_payload(payload: &Value) -> Option<&str> {
    payload.get("message_id").and_then(Value::as_str)
}

fn event_message(payload: &Value) -> Option<String> {
    if let Some(message) = payload.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(message.to_string());
    }
    for key in ["message", "error"] {
        if let Some(message) = payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(message.to_string());
        }
    }
    None
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
