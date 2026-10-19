//! Authenticated, resumable adapters. SQLite, never a socket, owns runtime facts.
use std::{collections::BTreeMap, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        OriginalUri, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::time::{timeout, Instant};
use uuid::Uuid;

use super::{
    account,
    auth::{self, AuthenticatedSession},
    error::ApiError,
    AppState,
};

pub const PROTOCOL: &str = "qunica.realtime.v2";

pub async fn ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let session = auth::current_session(&headers, &state).await?;
    let raw = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let server = account::server_id(state.db.pool()).await?;
    sqlx::query("INSERT INTO realtime_tickets(ticket_hash,server_id,owner_id,session_id,credential_id,expires_at) VALUES(?,?,?,?,?,unixepoch()+30)")
        .bind(hash(&raw)).bind(&server).bind(&session.owner_id).bind(&session.session_id).bind(&session.credential_id)
        .execute(state.db.pool()).await.map_err(|_|ApiError::internal("ticket creation failed"))?;
    Ok(Json(
        json!({"ticket":raw,"server_id":server,"expires_in":30,"protocol":PROTOCOL}),
    ))
}

fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

pub(crate) async fn consume_ticket(
    state: &AppState,
    raw: &str,
) -> Result<AuthenticatedSession, ApiError> {
    if raw.len() != 64 {
        return Err(ApiError::unauthorized("invalid realtime ticket"));
    }
    let mut tx = state
        .db
        .begin_write()
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    let row: Option<(String,String,String)> = sqlx::query_as("DELETE FROM realtime_tickets WHERE ticket_hash=? AND expires_at>unixepoch() AND server_id=(SELECT server_id FROM server_identity WHERE singleton=1) RETURNING owner_id,session_id,credential_id")
        .bind(hash(raw)).fetch_optional(&mut *tx).await.map_err(|_|ApiError::internal("database error"))?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    let (owner_id, session_id, credential_id) =
        row.ok_or_else(|| ApiError::unauthorized("expired or consumed realtime ticket"))?;
    let session = AuthenticatedSession {
        owner_id,
        session_id,
        credential_id,
    };
    if !auth::session_is_active(state.db.pool(), &session).await? {
        return Err(ApiError::unauthorized("device session was revoked"));
    }
    Ok(session)
}

pub async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if headers.contains_key("authorization") {
        auth::current_session(&headers, &state).await?;
    }
    validate_upgrade(&headers, &uri, PROTOCOL)?;
    Ok(ws
        .protocols([PROTOCOL])
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(4 * 1024 * 1024)
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| connection(socket, state)))
}

pub(crate) fn validate_upgrade(
    headers: &HeaderMap,
    uri: &axum::http::Uri,
    protocol: &str,
) -> Result<(), ApiError> {
    if uri.query().is_some() {
        return Err(ApiError::invalid_input(
            "realtime credentials belong in the first frame",
        ));
    }
    if !headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|p| p.trim() == protocol))
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_protocol",
            format!("Use {protocol}"),
        ));
    }
    if let Some(origin) = headers.get("origin") {
        let origin_text = origin
            .to_str()
            .map_err(|_| ApiError::permission_denied("invalid origin"))?;
        let origin_url = reqwest::Url::parse(origin_text)
            .map_err(|_| ApiError::permission_denied("invalid origin"))?;
        if origin_text != "tauri://localhost"
            && (!matches!(origin_url.scheme(), "http" | "https")
                || origin_url.path() != "/"
                || !origin_url.username().is_empty()
                || origin_url.password().is_some()
                || origin_url.query().is_some()
                || origin_url.fragment().is_some())
        {
            return Err(ApiError::permission_denied("invalid origin"));
        }
        let configured = super::parse_allowed_origins(
            &std::env::var("QUNICA_ALLOWED_ORIGINS").unwrap_or_default(),
        )
        .map_err(|_| ApiError::internal("invalid allowed origins"))?;
        let host = headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let same_origin = matches!(origin_url.scheme(), "http" | "https")
            && origin_url.origin().ascii_serialization()
                == format!("{}://{}", origin_url.scheme(), host);
        if !same_origin
            && !super::is_allowed_origin(origin)
            && !configured.iter().any(|v| v == origin_text)
        {
            return Err(ApiError::permission_denied(
                "realtime origin is not allowed",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn send(socket: &mut WebSocket, value: Value) -> Result<(), ()> {
    let raw = value.to_string();
    if raw.len() > 4 * 1024 * 1024 {
        return Err(());
    }
    timeout(Duration::from_secs(2), socket.send(Message::Text(raw)))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

async fn connection(mut socket: WebSocket, state: AppState) {
    let Ok(server) = account::server_id(state.db.pool()).await else {
        return;
    };
    if send(&mut socket,json!({"type":"hello","server_id":server,"protocol":PROTOCOL,"authenticate_within_ms":5000})).await.is_err(){return;}
    let session = match timeout(Duration::from_secs(5), socket.next()).await {
        Ok(Some(Ok(Message::Text(raw)))) => {
            let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            if value["type"] != "authenticate" {
                let _ = send(
                    &mut socket,
                    json!({"type":"error","code":"authentication_required"}),
                )
                .await;
                return;
            }
            match consume_ticket(&state, value["ticket"].as_str().unwrap_or("")).await {
                Ok(session) => session,
                Err(_) => {
                    let _ = send(&mut socket, json!({"type":"error","code":"unauthorized"})).await;
                    return;
                }
            }
        }
        _ => return,
    };
    if send(
        &mut socket,
        json!({"type":"authenticated","session_id":session.session_id}),
    )
    .await
    .is_err()
    {
        return;
    }
    let mut subscriptions: BTreeMap<String, i64> = BTreeMap::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut last_auth = Instant::now();
    let mut last_ping = Instant::now();
    let mut last_pong = Instant::now();
    loop {
        tokio::select! {
            incoming=socket.next()=> {
                let Some(Ok(message))=incoming else {break;};
                let Message::Text(raw)=message else {if matches!(message,Message::Close(_)){break;} continue;};
                let value: Value=match serde_json::from_str(&raw){Ok(v)=>v,Err(_)=>break};
                if !auth::session_is_active(state.db.pool(),&session).await.unwrap_or(false){break;}
                let result = handle_frame(&state,&session,&server,&mut subscriptions,&value).await;
                if value["type"]=="pong" {last_pong=Instant::now();}
                if send(&mut socket,result).await.is_err(){break;}
            }
            _=tick.tick()=> {
                if last_auth.elapsed()>=Duration::from_millis(250){
                    if !auth::session_is_active(state.db.pool(),&session).await.unwrap_or(false){break;}
                    last_auth=Instant::now();
                }
                if last_pong.elapsed()>=Duration::from_secs(60){break;}
                if last_ping.elapsed()>=Duration::from_secs(20){
                    if send(&mut socket,json!({"type":"ping"})).await.is_err(){break;}
                    last_ping=Instant::now();
                }
                let mut failed=false;
                for (scope,cursor) in subscriptions.iter_mut(){
                    let batch=if scope=="account" {
                        account::events_after(state.db.pool(),&session.owner_id,*cursor).await.map(|events|events.into_iter().map(|e|(e.seq,e.wire())).collect())
                    } else {run_after(&state,&session.owner_id,scope.strip_prefix("run:").unwrap_or(""),*cursor).await};
                    match batch {
                        // Note: one frame per poll, not per event. A long ACP run
                        // persists thousands of events; a frame each meant as many
                        // browser tasks, store updates, and acks (each ack re-checks
                        // the session in SQLite), which froze the UI.
                        Ok(events)=>if let Some((seq,_))=events.last() {
                            let seq=*seq;
                            let events:Vec<Value>=events.into_iter().map(|(_,event)|event).collect();
                            if send(&mut socket,json!({"type":"events","scope":scope,"cursor":seq.to_string(),"events":events})).await.is_err(){failed=true;}
                            else {*cursor=seq;}
                        },
                        Err(error)=>{let _=send(&mut socket,json!({"type":"resync_required","scope":scope,"message":error.message_text()})).await;failed=true;break;}
                    }
                    if failed {break;}
                }
                if failed {break;}
            }
        }
    }
    let _ = timeout(Duration::from_secs(1), socket.close()).await;
}

async fn handle_frame(
    state: &AppState,
    session: &AuthenticatedSession,
    server: &str,
    subscriptions: &mut BTreeMap<String, i64>,
    value: &Value,
) -> Value {
    match value["type"].as_str().unwrap_or("") {
        "ping" => json!({"type":"pong"}),
        "pong" => json!({"type":"ack","scope":"heartbeat"}),
        "ack" => json!({"type":"ack","scope":value["scope"],"cursor":value["cursor"]}),
        "unsubscribe" => {
            subscriptions.remove(value["scope"].as_str().unwrap_or(""));
            json!({"type":"unsubscribed","scope":value["scope"]})
        }
        "subscribe" => {
            let scope = value["scope"].as_str().unwrap_or("");
            if value["server_id"].as_str() != Some(server) {
                return json!({"type":"error","code":"server_mismatch"});
            }
            if subscriptions.len() >= 32 && !subscriptions.contains_key(scope) {
                return json!({"type":"error","code":"subscription_limit"});
            }
            let raw_cursor =
                value["cursor"]
                    .as_str()
                    .unwrap_or(if scope == "account" { "0" } else { "-1" });
            let cursor = match if scope != "account" && raw_cursor == "-1" {
                Ok(-1)
            } else {
                account::parse_cursor(raw_cursor)
            } {
                Ok(c) => c,
                Err(_) => return json!({"type":"error","code":"invalid_cursor"}),
            };
            let valid = if scope == "account" {
                account::events_after(state.db.pool(), &session.owner_id, cursor)
                    .await
                    .map(|_| ())
            } else if let Some(run) = scope.strip_prefix("run:") {
                run_after(state, &session.owner_id, run, cursor)
                    .await
                    .map(|_| ())
            } else {
                return json!({"type":"error","code":"unsupported_scope"});
            };
            if let Err(error) = valid {
                return json!({"type":if error.code()=="resync_required"{"resync_required"}else{"error"},"scope":scope,"code":error.code(),"message":error.message_text()});
            }
            subscriptions.insert(scope.to_string(), cursor);
            json!({"type":"subscribed","scope":scope,"cursor":cursor.to_string()})
        }
        "command" => super::commands::ws_command(state, session, value.clone()).await,
        _ => json!({"type":"error","code":"unsupported_message"}),
    }
}

async fn run_after(
    state: &AppState,
    owner: &str,
    run: &str,
    after: i64,
) -> Result<Vec<(i64, Value)>, ApiError> {
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    let rows: Vec<(i64,String,String,String)> = sqlx::query_as("SELECT e.seq,e.event_id,e.kind,e.payload_json FROM stream_events e JOIN threads t ON t.id=e.thread_id JOIN groups g ON g.id=t.group_id WHERE g.owner_id=? AND g.status='active' AND t.status!='cleared' AND e.stream_id=? AND e.seq>? ORDER BY e.seq LIMIT 256")
        .bind(owner).bind(run).bind(after).fetch_all(&mut *tx).await.map_err(|_|ApiError::internal("database error"))?;
    let exists: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_events e JOIN threads t ON t.id=e.thread_id JOIN groups g ON g.id=t.group_id WHERE e.stream_id=? AND g.owner_id=? AND g.status='active' AND t.status!='cleared') OR EXISTS(SELECT 1 FROM execution_jobs j JOIN groups g ON g.id=j.group_id JOIN threads t ON t.id=j.thread_id WHERE j.stream_id=? AND j.owner_id=? AND g.status='active' AND t.status!='cleared')")
        .bind(run).bind(owner).bind(run).bind(owner).fetch_one(&mut *tx).await.map_err(|_|ApiError::internal("database error"))?;
    let floor:Option<i64>=sqlx::query_scalar("SELECT r.retained_after FROM run_replay_floors r JOIN groups g ON g.id=r.group_id JOIN threads t ON t.id=r.thread_id WHERE r.stream_id=? AND g.owner_id=? AND g.status='active' AND t.status!='cleared'").bind(run).bind(owner).fetch_optional(&mut *tx).await.map_err(|_|ApiError::internal("database error"))?;
    if !exists && floor.is_none() {
        return Err(ApiError::not_found("run not found"));
    }
    let high: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(seq),-1) FROM stream_events WHERE stream_id=?")
            .bind(run)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ApiError::internal("database error"))?;
    if after < floor.unwrap_or(-1) || after > high.max(floor.unwrap_or(-1)) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "resync_required",
            "Run cursor is outside the replay window",
        ));
    }
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    Ok(rows.into_iter().map(|(seq,id,kind,payload)| (seq,json!({"type":"event","scope":format!("run:{run}"),"cursor":seq.to_string(),"event_id":id,"stream_id":run,"seq":seq,"kind":kind,"payload":serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null)}))).collect())
}
