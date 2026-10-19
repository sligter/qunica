//! A separate, authenticated RFB channel; pixels never enter account events.
use super::{auth, computers, error::ApiError, realtime, AppState};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        OriginalUri, Path, State,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;
const PROTOCOL: &str = "qunica.desktop.v1";
pub async fn control(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    computers::owned(&state, &owner, &id.to_string()).await?;
    if !matches!(
        body["action"].as_str(),
        Some("status" | "request" | "renew" | "release" | "resume")
    ) {
        return Err(ApiError::invalid_input("Unknown control action"));
    }
    let client = state
        .computers
        .as_ref()
        .ok_or_else(|| ApiError::conflict("Supervisor unavailable"))?;
    let result = client
        .request(
            reqwest::Method::POST,
            &format!("/v1/computers/{id}/control"),
            Some(&body),
        )
        .await;
    if body["action"] != "status" && body["action"] != "renew" {
        sqlx::query("UPDATE computers SET revision=revision+1 WHERE id=? AND owner_id=?")
            .bind(id.to_string())
            .bind(owner)
            .execute(state.db.pool())
            .await
            .map_err(|_| ApiError::internal("Computer state unavailable"))?;
    }
    Ok(Json(result?))
}
pub async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    realtime::validate_upgrade(&headers, &uri, PROTOCOL)?;
    Ok(ws
        .protocols([PROTOCOL])
        .max_message_size(96 * 1024)
        .max_frame_size(96 * 1024)
        .on_upgrade(move |socket| connection(socket, state)))
}
async fn connection(mut socket: WebSocket, state: AppState) {
    let frame = match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
        Ok(Some(Ok(Message::Text(raw)))) => {
            serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)
        }
        _ => return,
    };
    let Ok(session) =
        realtime::consume_ticket(&state, frame["ticket"].as_str().unwrap_or("")).await
    else {
        return;
    };
    let Some(client) = state.computers.as_ref() else {
        return;
    };
    let Some(id) = frame["computer_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return;
    };
    if computers::owned(&state, &session.owner_id, &id.to_string())
        .await
        .is_err()
    {
        return;
    }
    let generation = frame["generation"].as_str().unwrap_or("").to_string();
    let credential = frame["credential"].as_str().map(str::to_string);
    let path = format!("/v1/computers/{id}/call");
    let call = |request: Value| json!({"expected_generation":generation,"credential":credential,"request":request});
    let Ok(value) = client
        .request(
            reqwest::Method::POST,
            &path,
            Some(&call(json!({"op":"rfb_open"}))),
        )
        .await
    else {
        return;
    };
    let value: Value =
        serde_json::from_str(value["stdout"].as_str().unwrap_or("")).unwrap_or(Value::Null);
    let Some(display) = value["id"].as_str() else {
        return;
    };
    if socket
        .send(Message::Text(json!({"type":"authenticated"}).to_string()))
        .await
        .is_err()
    {
        return;
    }
    let mut timer = tokio::time::interval(Duration::from_millis(75));
    loop {
        let mut data = None;
        tokio::select! {
            incoming=socket.next()=>match incoming {Some(Ok(Message::Binary(bytes)))=>data=Some(base64::engine::general_purpose::STANDARD.encode(bytes)),Some(Ok(Message::Ping(bytes)))=>{let _=socket.send(Message::Pong(bytes)).await;continue;},_=>break},
            _=timer.tick()=>{},
        }
        if !auth::session_is_active(state.db.pool(), &session)
            .await
            .unwrap_or(false)
        {
            break;
        }
        let result = client
            .request(
                reqwest::Method::POST,
                &path,
                Some(&call(json!({"op":"rfb_exchange","id":display,"data":data}))),
            )
            .await;
        let value = match result {
            Ok(value) => value,
            Err(error) if error.code() == "computer_busy" && data.is_none() => continue,
            Err(_) => break,
        };
        if value["exit_code"] != 0 || value["truncated"] == true {
            break;
        }
        let value: Value =
            serde_json::from_str(value["stdout"].as_str().unwrap_or("")).unwrap_or(Value::Null);
        let Some(raw) = value["data"].as_str() else {
            break;
        };
        if !raw.is_empty() {
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) else {
                break;
            };
            if tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Binary(bytes)))
                .await
                .is_err()
            {
                break;
            }
        }
        if value["closed"] == true {
            break;
        }
    }
    let _ = client
        .request(
            reqwest::Method::POST,
            &path,
            Some(&call(json!({"op":"rfb_close","id":display}))),
        )
        .await;
    let _ = socket.close().await;
}
pub async fn artifacts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    computers::owned(&state, &owner, &id.to_string()).await?;
    let rows:Vec<(String,String,String,i64)>=sqlx::query_as("SELECT id,turn_id,path,size FROM computer_artifacts WHERE computer_id=? AND owner_id=? ORDER BY rowid DESC LIMIT 200").bind(id.to_string()).bind(owner).fetch_all(state.db.pool()).await.map_err(|_|ApiError::internal("Artifacts unavailable"))?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,turn,path,size)|json!({"id":id,"turn_id":turn,"path":path,"size":size})).collect::<Vec<_>>()}),
    ))
}
pub async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let (computer, path, size): (String, String, i64) = sqlx::query_as(
        "SELECT computer_id,path,size FROM computer_artifacts WHERE id=? AND owner_id=?",
    )
    .bind(id.to_string())
    .bind(owner)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("Artifact unavailable"))?
    .ok_or_else(|| ApiError::not_found("Artifact not found"))?;
    if !(0..=8 * 1024 * 1024).contains(&size) {
        return Err(ApiError::invalid_input("Artifact exceeds download limit"));
    }
    let client = state
        .computers
        .as_ref()
        .ok_or_else(|| ApiError::conflict("Supervisor unavailable"))?;
    let snapshot = client
        .request(
            reqwest::Method::GET,
            &format!("/v1/computers/{computer}"),
            None,
        )
        .await?;
    let mut bytes = Vec::new();
    while (bytes.len() as i64) < size {
        let value=client.request(reqwest::Method::POST,&format!("/v1/computers/{computer}/call"),Some(&json!({"expected_generation":snapshot["generation"],"request":{"op":"artifact","path":path,"offset":bytes.len()}}))).await?;
        if value["exit_code"] != 0 || value["truncated"] == true {
            return Err(ApiError::conflict("Artifact read failed"));
        }
        let value: Value = serde_json::from_str(value["stdout"].as_str().unwrap_or(""))
            .map_err(|_| ApiError::internal("Artifact read failed"))?;
        let chunk = base64::engine::general_purpose::STANDARD
            .decode(value["data"].as_str().unwrap_or(""))
            .map_err(|_| ApiError::internal("Artifact read failed"))?;
        if chunk.is_empty() || bytes.len() + chunk.len() > size as usize {
            return Err(ApiError::conflict("Artifact changed; refresh its metadata"));
        }
        bytes.extend(chunk);
    }
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("content-disposition", "attachment"),
            ("cache-control", "no-store"),
        ],
        bytes,
    )
        .into_response())
}
