//! PTY bytes use a separate socket and a per-connection input lease.
use super::{account, auth, error::ApiError, realtime, terminal, AppState};
use crate::terminal::{TerminalEvent, TerminalSession};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        OriginalUri, State,
    },
    http::HeaderMap,
    response::Response,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::time::{timeout, Instant};
use uuid::Uuid;

pub const PROTOCOL: &str = "qunica.terminal.v1";

pub async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if headers.contains_key("authorization") {
        auth::current_session(&headers, &state).await?;
    }
    realtime::validate_upgrade(&headers, &uri, PROTOCOL)?;
    Ok(ws
        .protocols([PROTOCOL])
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(4 * 1024 * 1024)
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| connection(socket, state)))
}

struct LeaseGuard {
    session: Arc<TerminalSession>,
    holder: String,
}
impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let session = self.session.clone();
        let holder = self.holder.clone();
        tokio::task::spawn_blocking(move || session.release_input(&holder));
    }
}

async fn connection(mut socket: WebSocket, state: AppState) {
    let Ok(server_id) = account::server_id(state.db.pool()).await else {
        return;
    };
    if realtime::send(
        &mut socket,
        json!({"type":"hello","server_id":server_id,"protocol":PROTOCOL}),
    )
    .await
    .is_err()
    {
        return;
    }
    let frame = match timeout(Duration::from_secs(5), socket.next()).await {
        Ok(Some(Ok(Message::Text(raw)))) => {
            serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)
        }
        _ => return,
    };
    if frame["type"] != "authenticate" {
        return;
    }
    let auth = match realtime::consume_ticket(&state, frame["ticket"].as_str().unwrap_or("")).await
    {
        Ok(auth) => auth,
        Err(error) => {
            let _ = realtime::send(
                &mut socket,
                json!({"type":"error","code":error.code(),"message":error.message_text()}),
            )
            .await;
            return;
        }
    };
    let session_id = frame["session_id"].as_str().unwrap_or("");
    let session = match terminal::require_session(&state, session_id, &auth.owner_id) {
        Ok(session) => session,
        Err(error) => {
            let _ = realtime::send(
                &mut socket,
                json!({"type":"error","code":error.code(),"message":error.message_text()}),
            )
            .await;
            return;
        }
    };
    let mut cursor = match frame["cursor"].as_str().unwrap_or("0").parse::<u64>() {
        Ok(cursor) => cursor,
        Err(_) => return,
    };
    if frame["repaint"] == true {
        cursor = session.replay_start();
    }
    let holder = Uuid::new_v4().to_string();
    let _guard = LeaseGuard {
        session: session.clone(),
        holder: holder.clone(),
    };
    let generation = if frame["request_input"] == false {
        None
    } else {
        session.acquire_input(&holder).map(|n| n.to_string())
    };
    if realtime::send(
        &mut socket,
        json!({"type":"authenticated","generation":generation,"lease_ms":15000,"cursor":cursor.to_string()}),
    )
    .await
    .is_err()
    {
        return;
    }
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut last_auth = Instant::now();
    let mut last_ping = Instant::now();
    let mut last_pong = Instant::now();
    loop {
        tokio::select! {
            incoming=socket.next()=> {
                let Some(Ok(Message::Text(raw)))=incoming else {break;};
                if !auth::session_is_active(state.db.pool(),&auth).await.unwrap_or(false) { break; }
                let Ok(value)=serde_json::from_str::<Value>(&raw) else {break;};
                let kind=value["type"].as_str().unwrap_or("");
                let request=value["request_id"].clone();
                let reply=match kind {
                    "pong" => {last_pong=Instant::now();continue;},
                    "ping" => json!({"type":"pong"}),
                    "acquire_input" => json!({"type":"lease","request_id":request,"generation":session.acquire_input(&holder).map(|n|n.to_string()),"lease_ms":15000}),
                    "release_input" => {
                        let target=session.clone();let holder=holder.clone();
                        if tokio::task::spawn_blocking(move||target.release_input(&holder)).await.is_err(){break;}
                        json!({"type":"lease","request_id":request,"generation":null})
                    },
                    "renew_input" => {
                        let generation=value["generation"].as_str().and_then(|s|s.parse::<u64>().ok()).unwrap_or(0);
                        let valid=session.input_lease_valid(&holder,generation);
                        json!({"type":"lease","request_id":request,"generation":if valid {session.acquire_input(&holder).map(|n|n.to_string())}else{None},"lease_ms":15000})
                    },
                    "input"|"resize"|"close"=>{
                        let generation=value["generation"].as_str().and_then(|s|s.parse::<u64>().ok()).unwrap_or(0);
                        let target=session.clone();let holder=holder.clone();
                        let data=value["data"].as_str().unwrap_or("").to_owned();
                        let cols=value["cols"].as_u64().unwrap_or(80).clamp(1,1000) as u16;
                        let rows=value["rows"].as_u64().unwrap_or(24).clamp(1,1000) as u16;
                        let kind=kind.to_owned();let manager=state.terminals.clone();let owner=auth.owner_id.clone();
                        let result=tokio::task::spawn_blocking(move || target.with_input_lease(&holder,generation,||{
                            match kind.as_str(){"input"=>target.write_input(data.as_bytes()),"resize"=>target.resize(cols,rows),_=>{manager.close(&target.id,&owner);Ok(())}}
                        })).await;
                        match result {Ok(Ok(()))=>json!({"type":"ack","request_id":request}),Ok(Err(error))=>json!({"type":"error","request_id":request,"code":error.code,"message":error.message}),Err(_)=>break}
                    },
                    _=>json!({"type":"error","request_id":request,"code":"unsupported_message"}),
                };
                if realtime::send(&mut socket,reply).await.is_err(){break;}
            },
            _=tick.tick()=>{
                if last_auth.elapsed()>=Duration::from_millis(250){
                    if !auth::session_is_active(state.db.pool(),&auth).await.unwrap_or(false){break;}
                    last_auth=Instant::now();
                }
                if last_pong.elapsed()>=Duration::from_secs(60){break;}
                if last_ping.elapsed()>=Duration::from_secs(20){if realtime::send(&mut socket,json!({"type":"ping"})).await.is_err(){break;}last_ping=Instant::now();}
                if state.terminals.get(&session.id,&auth.owner_id).is_none(){let _=realtime::send(&mut socket,json!({"type":"error","code":"terminal.session_missing","message":"Terminal session no longer exists. Open a new terminal."})).await;break;}
                match session.replay_after(cursor){
                    Err(error)=>{let _=realtime::send(&mut socket,json!({"type":"error","code":error.code,"message":error.message})).await;break;},
                    Ok(frames)=>{
                        let mut finished=false;
                        for (seq,event) in frames {
                            let exit=matches!(event,TerminalEvent::Exit{..});
                            if realtime::send(&mut socket,json!({"type":"output","cursor":seq.to_string(),"event":event})).await.is_err(){return;}
                            cursor=seq;
                            if exit {finished=true;break;}
                        }
                        if finished{break;}
                    }
                }
            }
        }
    }
    let _ = timeout(Duration::from_secs(1), socket.close()).await;
}
