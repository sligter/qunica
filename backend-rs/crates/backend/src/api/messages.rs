//! Group message streaming endpoint.
//!
//! `POST /api/v2/groups/{group_id}/messages/stream` validates the caller owns
//! an active group, then runs one turn of the [`crate::runtime`] in a spawned
//! task whose [`StreamEvent`]s are relayed to the client as Server-Sent Events.
//! When the client disconnects, the response body (and the channel receiver) is
//! dropped, the runtime's next send fails, and the turn stops.

use std::convert::Infallible;

use ag_swarmer_domain::events::StreamEvent;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::{stream::BoxStream, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    api::{auth::current_user_id, error::ApiError, AppState},
    runtime::{run_group_turn, RuntimeServices, TurnRequest},
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
        let data = serde_json::to_string(&event).unwrap_or_default();
        Some((Ok::<Event, Infallible>(Event::default().data(data)), rx))
    })
    .boxed();

    Ok(Sse::new(body))
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
