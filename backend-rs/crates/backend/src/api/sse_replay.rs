use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
use axum::http::HeaderMap;
use serde_json::Value;
use uuid::Uuid;

use super::error::ApiError;

#[derive(Debug)]
pub(crate) struct ReplayCursor {
    event_id: String,
    stream_id: Uuid,
    seq: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ReplayAnchorRow {
    stream_id: String,
    seq: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct StreamEventRow {
    stream_id: String,
    seq: i64,
    event_id: String,
    kind: String,
    payload_json: String,
}

pub(crate) fn last_event_id(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get("last-event-id") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::invalid_input("Last-Event-ID is invalid"))?
        .trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

pub(crate) fn parse_replay_cursor(raw: &str) -> Result<ReplayCursor, ApiError> {
    let (stream_id, seq) = raw
        .split_once(':')
        .ok_or_else(|| ApiError::invalid_input("Last-Event-ID is malformed"))?;
    if seq.contains(':') {
        return Err(ApiError::invalid_input("Last-Event-ID is malformed"));
    }
    let stream_id = Uuid::parse_str(stream_id)
        .map_err(|_| ApiError::invalid_input("Last-Event-ID is malformed"))?;
    let seq = seq
        .parse::<i64>()
        .map_err(|_| ApiError::invalid_input("Last-Event-ID is malformed"))?;
    if seq < 0 {
        return Err(ApiError::invalid_input("Last-Event-ID is malformed"));
    }
    Ok(ReplayCursor {
        event_id: raw.to_string(),
        stream_id,
        seq,
    })
}

pub(crate) async fn fetch_replay_events_for_group(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    cursor: &ReplayCursor,
) -> Result<Vec<StreamEvent<Value>>, ApiError> {
    let anchor: ReplayAnchorRow = sqlx::query_as(
        "SELECT se.stream_id, se.seq \
         FROM stream_events se \
         JOIN threads t ON t.id = se.thread_id \
         WHERE se.event_id = ? AND t.group_id = ?",
    )
    .bind(&cursor.event_id)
    .bind(group_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("stream event not found"))?;

    validate_anchor(&anchor, cursor)?;

    let rows: Vec<StreamEventRow> = sqlx::query_as(
        "SELECT se.stream_id, se.seq, se.event_id, se.kind, se.payload_json \
         FROM stream_events se \
         JOIN threads t ON t.id = se.thread_id \
         WHERE se.stream_id = ? AND se.seq > ? AND t.group_id = ? \
         ORDER BY se.seq ASC",
    )
    .bind(cursor.stream_id.to_string())
    .bind(cursor.seq)
    .bind(group_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    rows.into_iter().map(stream_event_from_row).collect()
}

pub(crate) async fn fetch_replay_events_for_stream(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    stream_id: Uuid,
) -> Result<Vec<StreamEvent<Value>>, ApiError> {
    let rows: Vec<StreamEventRow> = sqlx::query_as(
        "SELECT se.stream_id, se.seq, se.event_id, se.kind, se.payload_json \
         FROM stream_events se \
         JOIN threads t ON t.id = se.thread_id \
         WHERE se.stream_id = ? AND t.group_id = ? \
         ORDER BY se.seq ASC",
    )
    .bind(stream_id.to_string())
    .bind(group_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    rows.into_iter().map(stream_event_from_row).collect()
}

pub(crate) async fn fetch_replay_events_for_thread(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
    group_id: &str,
    cursor: &ReplayCursor,
) -> Result<Vec<StreamEvent<Value>>, ApiError> {
    let anchor: ReplayAnchorRow = sqlx::query_as(
        "SELECT se.stream_id, se.seq \
         FROM stream_events se \
         JOIN threads t ON t.id = se.thread_id \
         WHERE se.event_id = ? AND se.thread_id = ? AND t.group_id = ?",
    )
    .bind(&cursor.event_id)
    .bind(thread_id)
    .bind(group_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("stream event not found"))?;

    validate_anchor(&anchor, cursor)?;

    let rows: Vec<StreamEventRow> = sqlx::query_as(
        "SELECT se.stream_id, se.seq, se.event_id, se.kind, se.payload_json \
         FROM stream_events se \
         JOIN threads t ON t.id = se.thread_id \
         WHERE se.stream_id = ? AND se.seq > ? AND se.thread_id = ? AND t.group_id = ? \
         ORDER BY se.seq ASC",
    )
    .bind(cursor.stream_id.to_string())
    .bind(cursor.seq)
    .bind(thread_id)
    .bind(group_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    rows.into_iter().map(stream_event_from_row).collect()
}

pub(crate) fn event_kind_from_wire(raw: &str) -> Result<StreamEventKind, ApiError> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|_| ApiError::internal("runtime event kind was invalid"))
}

fn validate_anchor(anchor: &ReplayAnchorRow, cursor: &ReplayCursor) -> Result<(), ApiError> {
    if anchor.stream_id != cursor.stream_id.to_string() || anchor.seq != cursor.seq {
        return Err(ApiError::not_found("stream event not found"));
    }
    Ok(())
}

fn stream_event_from_row(row: StreamEventRow) -> Result<StreamEvent<Value>, ApiError> {
    let stream_id = Uuid::parse_str(&row.stream_id)
        .map_err(|_| ApiError::internal("runtime event stream id was invalid"))?;
    let kind = event_kind_from_wire(&row.kind)?;
    let payload = serde_json::from_str(&row.payload_json)
        .map_err(|_| ApiError::internal("runtime event payload was invalid"))?;
    Ok(StreamEvent {
        stream_id,
        seq: row.seq,
        event_id: row.event_id,
        kind,
        payload,
    })
}
