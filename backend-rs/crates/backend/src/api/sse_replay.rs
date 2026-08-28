use std::{collections::VecDeque, convert::Infallible, time::Duration};

use axum::{
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{
    stream::{self, BoxStream},
    StreamExt,
};
use qunica_domain::events::{StreamEvent, StreamEventKind};
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::error::ApiError;

const REPLAY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) type SseResponse = Sse<BoxStream<'static, Result<Event, Infallible>>>;

#[derive(Debug)]
pub(crate) struct ReplayCursor {
    event_id: String,
    stream_id: Uuid,
    seq: i64,
}

enum ReplayScope {
    Group(String),
    Thread { thread_id: String, group_id: String },
}

struct ReplayState {
    pool: SqlitePool,
    scope: ReplayScope,
    stream_id: Uuid,
    after_seq: i64,
    buffered: VecDeque<StreamEvent<Value>>,
    done: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct ReplayAnchorRow {
    stream_id: String,
    seq: i64,
    kind: String,
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

pub(crate) fn sse_response(events: BoxStream<'static, StreamEvent<Value>>) -> SseResponse {
    let body = events
        .map(|event| {
            let data = serde_json::to_string(&event).unwrap_or_default();
            Ok::<_, Infallible>(Event::default().id(event.event_id.clone()).data(data))
        })
        .boxed();
    Sse::new(body).keep_alive(
        KeepAlive::new()
            .interval(KEEP_ALIVE_INTERVAL)
            .text("keep-alive"),
    )
}

pub(crate) async fn replay_group_stream(
    pool: SqlitePool,
    group_id: String,
    cursor: ReplayCursor,
) -> Result<BoxStream<'static, StreamEvent<Value>>, ApiError> {
    let (events, done) = fetch_replay_events_for_group(&pool, &group_id, &cursor).await?;
    Ok(follow_replay(
        pool,
        ReplayScope::Group(group_id),
        cursor.stream_id,
        cursor.seq,
        events,
        done,
    ))
}

pub(crate) async fn replay_thread_stream(
    pool: SqlitePool,
    thread_id: String,
    group_id: String,
    cursor: ReplayCursor,
) -> Result<BoxStream<'static, StreamEvent<Value>>, ApiError> {
    let (events, done) =
        fetch_replay_events_for_thread(&pool, &thread_id, &group_id, &cursor).await?;
    Ok(follow_replay(
        pool,
        ReplayScope::Thread {
            thread_id,
            group_id,
        },
        cursor.stream_id,
        cursor.seq,
        events,
        done,
    ))
}

pub(crate) async fn replay_existing_stream(
    pool: SqlitePool,
    group_id: String,
    stream_id: Uuid,
) -> Result<Option<BoxStream<'static, StreamEvent<Value>>>, ApiError> {
    let events = fetch_replay_events_for_stream(&pool, &group_id, stream_id).await?;
    if events.is_empty() {
        return Ok(None);
    }
    let done = events
        .iter()
        .any(|event| event.kind == StreamEventKind::Done);
    Ok(Some(follow_replay(
        pool,
        ReplayScope::Group(group_id),
        stream_id,
        -1,
        events,
        done,
    )))
}

fn follow_replay(
    pool: SqlitePool,
    scope: ReplayScope,
    stream_id: Uuid,
    after_seq: i64,
    events: Vec<StreamEvent<Value>>,
    done: bool,
) -> BoxStream<'static, StreamEvent<Value>> {
    let state = ReplayState {
        pool,
        scope,
        stream_id,
        after_seq,
        buffered: events.into(),
        done,
    };
    // ponytail: DB polling is enough for reconnects; use a per-stream broadcast if replay load grows.
    stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.buffered.pop_front() {
                state.after_seq = event.seq;
                state.done = event.kind == StreamEventKind::Done;
                return Some((event, state));
            }
            if state.done {
                return None;
            }
            tokio::time::sleep(REPLAY_POLL_INTERVAL).await;
            state.buffered = fetch_replay_events_after(
                &state.pool,
                &state.scope,
                state.stream_id,
                state.after_seq,
            )
            .await
            .ok()?
            .into();
        }
    })
    .boxed()
}

pub(crate) async fn fetch_replay_events_for_group(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    cursor: &ReplayCursor,
) -> Result<(Vec<StreamEvent<Value>>, bool), ApiError> {
    let anchor: ReplayAnchorRow = sqlx::query_as(
        "SELECT se.stream_id, se.seq, se.kind \
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

    let events = fetch_replay_events_after(
        pool,
        &ReplayScope::Group(group_id.to_string()),
        cursor.stream_id,
        cursor.seq,
    )
    .await?;
    Ok((events, anchor.kind == "done"))
}

pub(crate) async fn fetch_replay_events_for_stream(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    stream_id: Uuid,
) -> Result<Vec<StreamEvent<Value>>, ApiError> {
    fetch_replay_events_after(
        pool,
        &ReplayScope::Group(group_id.to_string()),
        stream_id,
        -1,
    )
    .await
}

pub(crate) async fn fetch_replay_events_for_thread(
    pool: &sqlx::SqlitePool,
    thread_id: &str,
    group_id: &str,
    cursor: &ReplayCursor,
) -> Result<(Vec<StreamEvent<Value>>, bool), ApiError> {
    let anchor: ReplayAnchorRow = sqlx::query_as(
        "SELECT se.stream_id, se.seq, se.kind \
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

    let events = fetch_replay_events_after(
        pool,
        &ReplayScope::Thread {
            thread_id: thread_id.to_string(),
            group_id: group_id.to_string(),
        },
        cursor.stream_id,
        cursor.seq,
    )
    .await?;
    Ok((events, anchor.kind == "done"))
}

async fn fetch_replay_events_after(
    pool: &SqlitePool,
    scope: &ReplayScope,
    stream_id: Uuid,
    after_seq: i64,
) -> Result<Vec<StreamEvent<Value>>, ApiError> {
    let rows: Vec<StreamEventRow> = match scope {
        ReplayScope::Group(group_id) => {
            sqlx::query_as(
                "SELECT se.stream_id, se.seq, se.event_id, se.kind, se.payload_json \
             FROM stream_events se \
             JOIN threads t ON t.id = se.thread_id \
             WHERE se.stream_id = ? AND se.seq > ? AND t.group_id = ? \
             ORDER BY se.seq ASC",
            )
            .bind(stream_id.to_string())
            .bind(after_seq)
            .bind(group_id)
            .fetch_all(pool)
            .await
        }
        ReplayScope::Thread {
            thread_id,
            group_id,
        } => {
            sqlx::query_as(
                "SELECT se.stream_id, se.seq, se.event_id, se.kind, se.payload_json \
             FROM stream_events se \
             JOIN threads t ON t.id = se.thread_id \
             WHERE se.stream_id = ? AND se.seq > ? AND se.thread_id = ? AND t.group_id = ? \
             ORDER BY se.seq ASC",
            )
            .bind(stream_id.to_string())
            .bind(after_seq)
            .bind(thread_id)
            .bind(group_id)
            .fetch_all(pool)
            .await
        }
    }
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
