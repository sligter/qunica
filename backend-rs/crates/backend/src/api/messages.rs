//! Group message endpoints.
//!
//! The streaming endpoint relays runtime [`StreamEvent`]s as Server-Sent
//! Events. The non-stream send endpoint runs the same runtime while draining
//! its bounded channel, then shapes a frontend-compatible response from the
//! durable runtime events and persisted message rows.

use std::{collections::HashSet, convert::Infallible, fs, path::Path as FsPath};

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
        conversations::{ensure_active_owned_conversation, ConversationKind},
        error::ApiError,
        sse_replay::{
            event_kind_from_wire, fetch_replay_events_for_group, last_event_id, parse_replay_cursor,
        },
        AppState,
    },
    runtime::{
        group::{AttachmentKind, MessageAttachment},
        run_group_turn, RuntimeServices, TurnOutcome, TurnRequest,
    },
    tools::resolve_workspace_path,
};

/// Buffered events between the runtime task and the SSE response body. Bounded
/// so a slow/absent client applies backpressure (and so disconnects surface as
/// a failed send rather than unbounded growth).
const CHANNEL_CAPACITY: usize = 64;
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;

#[derive(Debug, Deserialize)]
pub struct MessageInput {
    content: String,
    #[serde(default)]
    attachments: Vec<MessageAttachmentInput>,
    #[serde(default)]
    thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageAttachmentInput {
    path: String,
}

pub type StreamRequest = MessageInput;
pub type SendRequest = MessageInput;

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
    attachments: Vec<MessageAttachment>,
    status: String,
    refs: Option<Value>,
    context_usage: Option<Value>,
    reasoning: Option<Value>,
    tool_calls: Option<Value>,
    turn_id: Option<String>,
    dispatch_id: Option<String>,
    reply_to_message_id: Option<String>,
    turn_summary: Option<TurnSummaryResponse>,
    created_at: String,
}

/// A compact scheduler terminal state attached only to the user message that
/// created a bounded turn. History consumers can render a summary without
/// fetching a trace for every message.
#[derive(Debug, Serialize)]
struct TurnSummaryResponse {
    status: String,
    termination_reason: Option<String>,
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
    turn_id: Option<String>,
    dispatch_id: Option<String>,
    reply_to_message_id: Option<String>,
    turn_status: Option<String>,
    turn_termination_reason: Option<String>,
    created_at: String,
}

impl From<MessageRow> for MessageResponse {
    fn from(row: MessageRow) -> Self {
        let parsed: Option<Value> = row
            .content_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok());
        let context_usage = parsed
            .as_ref()
            .and_then(|value| match value.get("context_usage") {
                Some(Value::Object(_)) => value.get("context_usage").cloned(),
                _ => None,
            });
        let reasoning = parsed
            .as_ref()
            .and_then(|value| match value.get("reasoning") {
                Some(Value::Array(items)) if !items.is_empty() => value.get("reasoning").cloned(),
                _ => None,
            });
        let tool_calls = parsed
            .as_ref()
            .and_then(|value| match value.get("tool_calls") {
                Some(Value::Array(items)) if !items.is_empty() => value.get("tool_calls").cloned(),
                _ => None,
            });
        let attachments = parsed
            .as_ref()
            .and_then(|value| serde_json::from_value(value["attachments"].clone()).ok())
            .unwrap_or_default();
        Self {
            id: row.id,
            group_id: row.group_id,
            thread_id: Some(row.thread_id),
            sender_type: row.sender_type,
            sender_id: row.sender_id,
            message_type: row.message_type,
            content: row.content,
            attachments,
            status: row.status,
            refs: None,
            context_usage,
            reasoning,
            tool_calls,
            turn_id: row.turn_id,
            dispatch_id: row.dispatch_id,
            reply_to_message_id: row.reply_to_message_id,
            turn_summary: row.turn_status.map(|status| TurnSummaryResponse {
                status,
                termination_reason: row.turn_termination_reason,
            }),
            created_at: row.created_at,
        }
    }
}

pub async fn list_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    list_for_kind(state, headers, group_id, query, ConversationKind::Group).await
}

pub async fn list_direct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    list_for_kind(state, headers, group_id, query, ConversationKind::Direct).await
}

async fn list_for_kind(
    state: AppState,
    headers: HeaderMap,
    group_id: String,
    query: ListMessagesQuery,
    expected: ConversationKind,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let limit = parse_limit(query.limit.as_deref())?;
    let before_id = query
        .before
        .as_deref()
        .map(|raw| validate_uuid(raw, "before message id"))
        .transpose()?;

    ensure_active_owned_conversation(state.db.pool(), &group_id, &owner_id, expected).await?;

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

pub async fn clear_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<ClearMessagesResponse>, ApiError> {
    clear_for_kind(state, headers, group_id, ConversationKind::Group).await
}

pub async fn clear_direct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<ClearMessagesResponse>, ApiError> {
    clear_for_kind(state, headers, group_id, ConversationKind::Direct).await
}

async fn clear_for_kind(
    state: AppState,
    headers: HeaderMap,
    group_id: String,
    expected: ConversationKind,
) -> Result<Json<ClearMessagesResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    ensure_active_owned_conversation(state.db.pool(), &group_id, &owner_id, expected).await?;

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

pub async fn delete_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, message_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    delete_for_kind(
        state,
        headers,
        group_id,
        message_id,
        ConversationKind::Group,
    )
    .await
}

pub async fn delete_direct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, message_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    delete_for_kind(
        state,
        headers,
        group_id,
        message_id,
        ConversationKind::Direct,
    )
    .await
}

async fn delete_for_kind(
    state: AppState,
    headers: HeaderMap,
    group_id: String,
    message_id: String,
    expected: ConversationKind,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let message_id = validate_uuid(&message_id, "message id")?;

    ensure_active_owned_conversation(state.db.pool(), &group_id, &owner_id, expected).await?;

    let _guard = state.write_lock.lock().await;
    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start message delete transaction"))?;

    let row = sqlx::query_as::<_, (String,)>(
        "SELECT thread_id FROM messages \
         WHERE id = ? AND group_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(&message_id)
    .bind(&group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("message not found"))?;
    let thread_id = row.0;

    let changed = sqlx::query(
        "UPDATE messages \
         SET status = 'cleared' \
         WHERE id = ? AND group_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(&message_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to delete message"))?
    .rows_affected();
    if changed == 0 {
        return Err(ApiError::not_found("message not found"));
    }

    sqlx::query(
        "UPDATE threads \
         SET status = 'cleared', updated_at = ? \
         WHERE id = ? \
           AND group_id = ? \
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
    .bind(&thread_id)
    .bind(&group_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update message thread"))?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit message delete"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn send_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<SendRequest>,
) -> Result<(StatusCode, Json<MessageSendResponse>), ApiError> {
    send_for_kind(state, headers, group_id, body, ConversationKind::Group).await
}

pub async fn send_direct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<SendRequest>,
) -> Result<(StatusCode, Json<MessageSendResponse>), ApiError> {
    send_for_kind(state, headers, group_id, body, ConversationKind::Direct).await
}

async fn send_for_kind(
    state: AppState,
    headers: HeaderMap,
    group_id: String,
    body: SendRequest,
    expected: ConversationKind,
) -> Result<(StatusCode, Json<MessageSendResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let content = body.content.trim().to_string();
    ensure_active_owned_conversation(state.db.pool(), &group_id, &owner_id, expected).await?;
    if expected == ConversationKind::Direct {
        ensure_direct_agent_available(state.db.pool(), &group_id, &owner_id).await?;
    }
    let attachments =
        validate_attachments(state.db.pool(), &group_id, &owner_id, body.attachments).await?;
    if content.is_empty() && attachments.is_empty() {
        return Err(ApiError::invalid_input(
            "content or attachments must not be empty",
        ));
    }

    let (tx, mut rx) = mpsc::channel::<StreamEvent<Value>>(CHANNEL_CAPACITY);
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone())
        .with_active_turn_registry(state.active_turns.clone());
    let request = TurnRequest {
        group_id: group_id.clone(),
        owner_id,
        thread_id: body.thread_id,
        content,
        attachments,
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

pub async fn stream_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<StreamRequest>,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, ApiError> {
    stream_for_kind(state, headers, group_id, body, ConversationKind::Group).await
}

pub async fn stream_direct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<StreamRequest>,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, ApiError> {
    stream_for_kind(state, headers, group_id, body, ConversationKind::Direct).await
}

async fn stream_for_kind(
    state: AppState,
    headers: HeaderMap,
    group_id: String,
    body: StreamRequest,
    expected: ConversationKind,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let content = body.content.trim().to_string();
    ensure_active_owned_conversation(state.db.pool(), &group_id, &owner_id, expected).await?;
    if expected == ConversationKind::Direct {
        ensure_direct_agent_available(state.db.pool(), &group_id, &owner_id).await?;
    }
    let attachments =
        validate_attachments(state.db.pool(), &group_id, &owner_id, body.attachments).await?;
    if content.is_empty() && attachments.is_empty() {
        return Err(ApiError::invalid_input(
            "content or attachments must not be empty",
        ));
    }

    if let Some(cursor) = last_event_id(&headers)? {
        let cursor = parse_replay_cursor(&cursor)?;
        let events = fetch_replay_events_for_group(state.db.pool(), &group_id, &cursor).await?;
        let body = futures_util::stream::iter(events.into_iter().map(event_to_sse)).boxed();
        return Ok(Sse::new(body));
    }

    let (tx, rx) = mpsc::channel::<StreamEvent<Value>>(CHANNEL_CAPACITY);
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone())
        .with_active_turn_registry(state.active_turns.clone());
    let request = TurnRequest {
        group_id,
        owner_id,
        thread_id: body.thread_id,
        content,
        attachments,
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
        "SELECT m.id, m.group_id, m.thread_id, m.sender_type, m.sender_id, m.message_type, \
                m.content, m.content_json, m.status, m.turn_id, m.dispatch_id, m.reply_to_message_id, \
                CASE WHEN gt.trigger_message_id = m.id THEN gt.status END AS turn_status, \
                CASE WHEN gt.trigger_message_id = m.id THEN gt.termination_reason END AS turn_termination_reason, \
                m.created_at \
         FROM messages m \
         LEFT JOIN group_turns gt ON gt.id = m.turn_id \
         WHERE m.id = ? AND m.group_id = ?",
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
                "SELECT m.id, m.group_id, m.thread_id, m.seq, m.sender_type, m.sender_id, m.message_type, \
                        m.content, m.content_json, m.status, m.turn_id, m.dispatch_id, m.reply_to_message_id, \
                        CASE WHEN gt.trigger_message_id = m.id THEN gt.status END AS turn_status, \
                        CASE WHEN gt.trigger_message_id = m.id THEN gt.termination_reason END AS turn_termination_reason, \
                        m.created_at \
                 FROM messages m \
                 LEFT JOIN group_turns gt ON gt.id = m.turn_id \
                 WHERE m.group_id = ? \
                   AND m.status IN ('visible', 'interrupted') \
                   AND (m.seq < ? OR (m.seq = ? AND m.id < ?)) \
                 ORDER BY m.seq DESC, m.id DESC \
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
                "SELECT m.id, m.group_id, m.thread_id, m.seq, m.sender_type, m.sender_id, m.message_type, \
                        m.content, m.content_json, m.status, m.turn_id, m.dispatch_id, m.reply_to_message_id, \
                        CASE WHEN gt.trigger_message_id = m.id THEN gt.status END AS turn_status, \
                        CASE WHEN gt.trigger_message_id = m.id THEN gt.termination_reason END AS turn_termination_reason, \
                        m.created_at \
                 FROM messages m \
                 LEFT JOIN group_turns gt ON gt.id = m.turn_id \
                 WHERE m.group_id = ? AND m.status IN ('visible', 'interrupted') \
                 ORDER BY m.seq DESC, m.id DESC \
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

async fn ensure_direct_agent_available(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let active: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM groups g JOIN agents a ON a.id = g.direct_agent_id \
         WHERE g.id = ? AND g.owner_id = ? AND g.conversation_kind = 'direct' \
           AND a.owner_id = ? AND a.status = 'active' LIMIT 1",
    )
    .bind(group_id)
    .bind(owner_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    active
        .map(|_| ())
        .ok_or_else(|| ApiError::conflict("direct chat agent is unavailable"))
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

async fn validate_attachments(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    owner_id: &str,
    inputs: Vec<MessageAttachmentInput>,
) -> Result<Vec<MessageAttachment>, ApiError> {
    if inputs.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(ApiError::invalid_input(format!(
            "at most {MAX_ATTACHMENTS_PER_MESSAGE} attachments are allowed"
        )));
    }
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let workspace: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT w.owner_id, w.backend_type, w.local_path, w.status \
         FROM groups g JOIN workspaces w ON w.id = g.workspace_id \
         WHERE g.id = ? AND g.owner_id = ? AND g.status = 'active'",
    )
    .bind(group_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    let Some((workspace_owner_id, backend_type, local_path, status)) = workspace else {
        return Err(ApiError::invalid_input(
            "conversation has no active workspace",
        ));
    };
    if workspace_owner_id != owner_id || status != "active" || backend_type != "local" {
        return Err(ApiError::invalid_input(
            "attachments require an active local workspace",
        ));
    }
    let root = local_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| ApiError::invalid_input("local workspace has no local_path"))?;
    let root = fs::canonicalize(root)
        .map_err(|_| ApiError::invalid_input("workspace path must be an existing directory"))?;
    if !root.is_dir() {
        return Err(ApiError::invalid_input(
            "workspace path must be an existing directory",
        ));
    }

    let mut seen = HashSet::new();
    let mut attachments = Vec::with_capacity(inputs.len());
    for input in inputs {
        let path = resolve_workspace_path(&root, &input.path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        let path = fs::canonicalize(&path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(ApiError::invalid_input(
                "attachment path must be a workspace file",
            ));
        }
        if !seen.insert(path.clone()) {
            return Err(ApiError::invalid_input("attachment paths must be unique"));
        }
        let metadata = fs::metadata(&path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?
            .to_string_lossy()
            .replace('\\', "/");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ApiError::invalid_input("attachment path is invalid"))?
            .to_string();
        let mime_type = attachment_content_type(FsPath::new(&path)).to_string();
        let kind = match mime_type.as_str() {
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" => AttachmentKind::Image,
            _ => AttachmentKind::File,
        };
        attachments.push(MessageAttachment {
            id: Uuid::new_v4().to_string(),
            path: relative,
            name,
            mime_type,
            size: metadata.len() as i64,
            kind,
        });
    }
    Ok(attachments)
}

fn attachment_content_type(path: &FsPath) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
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
