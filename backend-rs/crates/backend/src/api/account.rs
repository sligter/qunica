//! Durable account discovery shared by WebSocket and paired SSE clients.
use std::{collections::VecDeque, convert::Infallible, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::{auth::current_user_id, error::ApiError, AppState};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AccountEvent {
    pub event_id: String,
    #[serde(skip)]
    pub seq: i64,
    pub kind: String,
    pub resource_id: String,
    pub group_id: Option<String>,
    pub thread_id: Option<String>,
    pub occurred_at: String,
    #[serde(skip)]
    pub payload_json: String,
}

impl AccountEvent {
    pub fn wire(&self) -> Value {
        json!({"type":"event", "scope":"account", "event_id":self.event_id,
            "cursor":self.seq.to_string(), "resource_version":self.seq.to_string(),
            "kind":self.kind, "resource_id":self.resource_id, "group_id":self.group_id,
            "thread_id":self.thread_id, "occurred_at":self.occurred_at,
            "payload":serde_json::from_str::<Value>(&self.payload_json).unwrap_or(Value::Null)})
    }
}

pub async fn server_id(pool: &SqlitePool) -> Result<String, ApiError> {
    sqlx::query_scalar("SELECT server_id FROM server_identity WHERE singleton = 1")
        .fetch_one(pool)
        .await
        .map_err(db_error)
}

pub async fn capabilities(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"server_id":server_id(state.db.pool()).await?, "build_version":env!("CARGO_PKG_VERSION"),
        "protocol_versions":["qunica.realtime.v2"],"terminal_protocol_versions":["qunica.terminal.v1"], "transports":["websocket","account_sse","run_sse","terminal_websocket","terminal_sse"],
        "execution_environments":if state.computers.is_some(){vec!["local","docker_computer"]}else{vec!["local"]}, "path_prefix_supported":false}),
    ))
}

/// Note: the watermark is read before resource data in the same SQLite snapshot;
/// a concurrent writer belongs either entirely here or entirely to replay.
pub async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let mut tx = state.db.pool().begin().await.map_err(db_error)?;
    let watermark: i64 = sqlx::query_scalar("SELECT COALESCE((SELECT high_watermark FROM account_event_counters WHERE owner_id = ?), 0)")
        .bind(&owner).fetch_one(&mut *tx).await.map_err(db_error)?;
    let server: String =
        sqlx::query_scalar("SELECT server_id FROM server_identity WHERE singleton = 1")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
    let groups = json_rows(&mut tx, "SELECT json_object('id',id,'name',name,'conversation_kind',conversation_kind,'status',status,'updated_at',updated_at) FROM groups WHERE owner_id = ? AND status = 'active' ORDER BY id", &owner).await?;
    let threads = json_rows(&mut tx, "SELECT json_object('id',t.id,'group_id',t.group_id,'title',t.title,'status',t.status,'updated_at',t.updated_at) FROM threads t JOIN groups g ON g.id=t.group_id WHERE g.owner_id=? AND g.status='active' AND t.agent_id IS NULL AND t.status != 'cleared' ORDER BY t.id", &owner).await?;
    let turns = json_rows(&mut tx, "SELECT json_object('id',r.id,'group_id',r.group_id,'thread_id',r.thread_id,'status',r.status) FROM group_turns r JOIN groups g ON g.id=r.group_id WHERE g.owner_id=? AND g.status='active' AND r.status IN ('pending','running','waiting_for_user')", &owner).await?;
    let jobs = json_rows(&mut tx, "SELECT json_object('id',j.id,'group_id',j.group_id,'thread_id',j.thread_id,'turn_id',j.turn_id,'stream_id',j.stream_id,'status',j.status) FROM execution_jobs j JOIN groups g ON g.id=j.group_id WHERE j.owner_id=? AND g.status='active' AND j.status IN ('queued','running','waiting_for_user')", &owner).await?;
    let approvals = json_rows(&mut tx, "SELECT json_object('id',a.id,'group_id',a.group_id,'thread_id',a.thread_id,'tool_call_id',a.tool_call_id,'version',a.version,'action_hash',a.action_hash,'request',json(a.request_json)) FROM pending_approvals a JOIN groups g ON g.id=a.group_id WHERE a.owner_id=? AND g.status='active' AND a.status='pending'", &owner).await?;
    let versions = json_rows(&mut tx, "SELECT json_object('resource_id',resource_id,'version',CAST(version AS TEXT)) FROM resource_versions WHERE owner_id=?", &owner).await?;
    let blocked_jobs = json_rows(&mut tx, "SELECT json_object('id',j.id,'group_id',j.group_id,'thread_id',j.thread_id,'turn_id',j.turn_id) FROM execution_jobs j JOIN groups g ON g.id=j.group_id WHERE j.owner_id=? AND g.status='active' AND j.status='interrupted' AND COALESCE(j.error,'')!='effects_verified_stopped'", &owner).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(
        json!({"server_id":server,"cursor":watermark.to_string(),"groups":groups,
        "threads":threads,"active_turns":turns,"active_jobs":jobs,"blocked_jobs":blocked_jobs,"pending_approvals":approvals,"resource_versions":versions}),
    ))
}

async fn json_rows(
    tx: &mut Transaction<'_, Sqlite>,
    query: &str,
    owner: &str,
) -> Result<Vec<Value>, ApiError> {
    let rows: Vec<String> = sqlx::query_scalar(query)
        .bind(owner)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_error)?;
    rows.into_iter()
        .map(|raw| {
            serde_json::from_str(&raw).map_err(|_| ApiError::internal("invalid snapshot data"))
        })
        .collect()
}

#[derive(Deserialize)]
pub struct ThreadSnapshotQuery {
    pub before: Option<i64>,
}

pub async fn thread_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Query(query): Query<ThreadSnapshotQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let mut tx = state.db.pool().begin().await.map_err(db_error)?;
    let row: Option<(String,String)> = sqlx::query_as("SELECT t.group_id,t.status FROM threads t JOIN groups g ON g.id=t.group_id WHERE t.id=? AND g.owner_id=? AND g.status='active' AND t.agent_id IS NULL AND t.status != 'cleared'")
        .bind(&thread).bind(&owner).fetch_optional(&mut *tx).await.map_err(db_error)?;
    let (group, status) = row.ok_or_else(|| ApiError::not_found("thread not found"))?;
    let messages: Vec<String> = sqlx::query_scalar("SELECT json_object('id',id,'group_id',group_id,'thread_id',thread_id,'seq',CAST(seq AS TEXT),'sender_type',sender_type,'sender_id',sender_id,'content',content,'content_json',content_json,'status',status,'created_at',created_at) FROM messages WHERE thread_id=? AND status IN ('visible','interrupted') AND seq < ? ORDER BY seq DESC LIMIT 100")
        .bind(&thread).bind(query.before.unwrap_or(i64::MAX)).fetch_all(&mut *tx).await.map_err(db_error)?;
    let messages: Vec<Value> = messages
        .into_iter()
        .rev()
        .map(|raw| serde_json::from_str(&raw).unwrap_or(Value::Null))
        .collect();
    let runs: Vec<String> = sqlx::query_scalar("SELECT json_object('stream_id',stream_id,'cursor',CAST(MAX(seq) AS TEXT)) FROM stream_events WHERE thread_id=? GROUP BY stream_id")
        .bind(&thread).fetch_all(&mut *tx).await.map_err(db_error)?;
    let approvals: Vec<String> = sqlx::query_scalar("SELECT json_object('id',id,'tool_call_id',tool_call_id,'version',version,'action_hash',action_hash,'request',json(request_json)) FROM pending_approvals WHERE thread_id=? AND status='pending'")
        .bind(&thread).fetch_all(&mut *tx).await.map_err(db_error)?;
    // Note: each agent's last `agent_message` is found once per run and joined,
    // not looked up per token row; the correlated form rescanned the run for
    // every delta and grew quadratically with long ACP runs.
    let inflight:Vec<String>=sqlx::query_scalar("WITH live AS (SELECT e.stream_id,e.seq,e.kind,e.payload_json,json_extract(e.payload_json,'$.agent_id') AS agent_id FROM stream_events e JOIN execution_jobs j ON j.stream_id=e.stream_id WHERE e.thread_id=? AND j.status IN ('queued','running')), finals AS (SELECT stream_id,agent_id,MAX(seq) AS seq FROM live WHERE kind='agent_message' GROUP BY stream_id,agent_id) SELECT json_object('stream_id',stream_id,'agent_id',agent_id,'kind',kind,'seq',MAX(seq),'text',group_concat(COALESCE(json_extract(payload_json,'$.text'),json_extract(payload_json,'$.delta'),''),'')) FROM (SELECT l.* FROM live l LEFT JOIN finals f ON f.stream_id=l.stream_id AND f.agent_id=l.agent_id WHERE l.kind IN ('token','reasoning') AND l.seq>COALESCE(f.seq,-1) ORDER BY l.seq) GROUP BY stream_id,agent_id,kind")
        .bind(&thread).fetch_all(&mut *tx).await.map_err(db_error)?;
    let inflight_events:Vec<String>=sqlx::query_scalar("SELECT json_object('stream_id',e.stream_id,'seq',e.seq,'kind',e.kind,'event_id',e.event_id,'payload',json(e.payload_json)) FROM stream_events e JOIN execution_jobs j ON j.stream_id=e.stream_id WHERE e.thread_id=? AND j.status IN ('queued','running') AND e.kind NOT IN ('token','reasoning') ORDER BY e.stream_id,e.seq")
        .bind(&thread).fetch_all(&mut *tx).await.map_err(db_error)?;
    let jobs = json_rows(&mut tx, "SELECT json_object('id',id,'group_id',group_id,'thread_id',thread_id,'turn_id',turn_id,'stream_id',stream_id,'status',status) FROM execution_jobs WHERE thread_id=? AND status IN ('queued','running','waiting_for_user')", &thread).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(
        json!({"thread_id":thread,"group_id":group,"status":status,"messages":messages,
        "runs":runs.into_iter().map(|s|serde_json::from_str::<Value>(&s).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "inflight_output":inflight.into_iter().map(|s|serde_json::from_str::<Value>(&s).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "active_jobs":jobs,
        "inflight_events":inflight_events.into_iter().map(|s|serde_json::from_str::<Value>(&s).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "pending_approvals":approvals.into_iter().map(|s|serde_json::from_str::<Value>(&s).unwrap_or(Value::Null)).collect::<Vec<_>>() }),
    ))
}

pub fn parse_cursor(raw: &str) -> Result<i64, ApiError> {
    if raw.is_empty() || raw.len() > 19 || !raw.bytes().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::invalid_input(
            "cursor must be a nonnegative decimal string",
        ));
    }
    raw.parse()
        .map_err(|_| ApiError::invalid_input("cursor is out of range"))
}

pub async fn events_after(
    pool: &SqlitePool,
    owner: &str,
    after: i64,
) -> Result<Vec<AccountEvent>, ApiError> {
    let mut tx = pool.begin().await.map_err(db_error)?;
    let (high, floor): (i64,i64) = sqlx::query_as("SELECT COALESCE((SELECT high_watermark FROM account_event_counters WHERE owner_id=?),0), COALESCE((SELECT retained_after FROM account_event_counters WHERE owner_id=?),0)")
        .bind(owner).bind(owner).fetch_one(&mut *tx).await.map_err(db_error)?;
    if after < floor || after > high {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "resync_required",
            "Account cursor is outside the replay window",
        ));
    }
    let events=sqlx::query_as("SELECT seq,event_id,kind,resource_id,group_id,thread_id,payload_json,occurred_at FROM account_events WHERE owner_id=? AND seq>? ORDER BY seq LIMIT 256")
        .bind(owner).bind(after).fetch_all(&mut *tx).await.map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(events)
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub cursor: String,
    pub server_id: String,
}

pub async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    if query.server_id != server_id(state.db.pool()).await? {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "server_mismatch",
            "Snapshot belongs to another instance",
        ));
    }
    let cursor = headers
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or(&query.cursor);
    let after = parse_cursor(cursor)?;
    let buffer: VecDeque<_> = events_after(state.db.pool(), &owner, after).await?.into();
    let stream = stream::unfold(
        (state.db.pool().clone(), owner, after, buffer, false),
        |(pool, owner, mut after, mut buffer, done)| async move {
            if done {
                return None;
            }
            loop {
                if let Some(event) = buffer.pop_front() {
                    after = event.seq;
                    let wire = event.wire();
                    return Some((
                        Ok(Event::default()
                            .id(after.to_string())
                            .json_data(wire)
                            .unwrap()),
                        (pool, owner, after, buffer, false),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                match events_after(&pool, &owner, after).await {
                    Ok(events) => buffer = events.into(),
                    Err(_) => {
                        return Some((
                            Ok(Event::default().event("resync_required").data("{}")),
                            (pool, owner, after, buffer, true),
                        ))
                    }
                }
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))))
}

/// Keep a durable floor even after every retained row is deleted. A missing
/// notification or an empty replay window can never turn an expired cursor valid.
pub async fn prune(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = crate::db::begin_write(pool).await?;
    sqlx::query("UPDATE account_event_counters SET retained_after=MAX(retained_after,COALESCE((SELECT MAX(seq) FROM account_events e WHERE e.owner_id=account_event_counters.owner_id AND (e.occurred_at < strftime('%Y-%m-%dT%H:%M:%fZ','now','-7 days') OR e.seq <= high_watermark-100000)),0))")
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM account_events WHERE seq <= (SELECT retained_after FROM account_event_counters c WHERE c.owner_id=account_events.owner_id)").execute(&mut *tx).await?;
    sqlx::query("DELETE FROM realtime_tickets WHERE expires_at < unixepoch()")
        .execute(&mut *tx)
        .await?;
    let days = std::env::var("QUNICA_ENDED_RUN_EVENT_DAYS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(7)
        .max(1);
    let interval = format!("-{days} days");
    sqlx::query("INSERT INTO run_replay_floors(stream_id,group_id,thread_id,retained_after) SELECT e.stream_id,t.group_id,e.thread_id,MAX(e.seq) FROM stream_events e JOIN threads t ON t.id=e.thread_id WHERE e.created_at < strftime('%Y-%m-%dT%H:%M:%fZ','now',?) AND NOT EXISTS(SELECT 1 FROM execution_jobs j WHERE j.thread_id=e.thread_id AND j.status IN ('queued','running','waiting_for_user')) AND NOT EXISTS(SELECT 1 FROM pending_approvals a WHERE a.thread_id=e.thread_id AND a.status='pending') AND EXISTS(SELECT 1 FROM stream_events d WHERE d.stream_id=e.stream_id AND d.kind='done') GROUP BY e.stream_id ON CONFLICT(stream_id) DO UPDATE SET retained_after=MAX(retained_after,excluded.retained_after)")
        .bind(interval).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM stream_events WHERE seq<=(SELECT retained_after FROM run_replay_floors r WHERE r.stream_id=stream_events.stream_id)")
        .execute(&mut *tx).await?;
    tx.commit().await
}

fn db_error(_: sqlx::Error) -> ApiError {
    ApiError::internal("database error")
}
