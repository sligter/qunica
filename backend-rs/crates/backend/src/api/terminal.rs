//! HTTP surface for server-owned terminal sessions.
//!
//! The desktop shell talks to its own PTY over Tauri commands; a browser
//! cannot, so these routes give the web build the same capability. Output is a
//! one-way SSE stream, input and resizing are ordinary requests.
//!
//! Every session belongs to the account that created it and may only start
//! inside that account's workspaces, so a terminal never reaches further into
//! the host than the file tools already do.

use std::{collections::VecDeque, convert::Infallible, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::{self, BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{error::RecvError, Receiver};

use crate::{
    api::{auth::current_user_id, error::ApiError, system_settings, workspaces, AppState},
    terminal::{
        CreateTerminalOptions, TerminalDescriptor, TerminalError, TerminalEvent, TerminalFrame,
        TerminalSession,
    },
    tools::ShellPreference,
};

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
/// Guards against a paste large enough to stall the PTY writer.
const MAX_INPUT_BYTES: usize = 256 * 1024;
const MAX_DIMENSION: u16 = 1000;

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    conversation_id: String,
    cwd: String,
    cols: u16,
    rows: u16,
    /// Overrides the account's shell preference for this session.
    #[serde(default)]
    shell: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InputRequest {
    data: String,
}

#[derive(Debug, Deserialize)]
pub struct ResizeRequest {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Serialize)]
pub struct ClosedResponse {
    closed: usize,
}

type SseResponse = Sse<BoxStream<'static, Result<Event, Infallible>>>;

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<TerminalDescriptor>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let cwd = authorize_cwd(&state, &owner_id, &body.cwd).await?;
    let shell = match body
        .shell
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => ShellPreference::parse(raw)
            .ok_or_else(|| ApiError::invalid_input("unknown shell preference"))?,
        None => system_settings::shell_preference(state.db.pool(), &owner_id)
            .await
            .unwrap_or_default(),
    };

    let session = state
        .terminals
        .create(CreateTerminalOptions {
            owner_id,
            conversation_id: body.conversation_id,
            cwd,
            cols: clamp_dimension(body.cols, 80),
            rows: clamp_dimension(body.rows, 24),
            shell,
        })
        .map_err(terminal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(TerminalDescriptor {
            session_id: session.id.clone(),
            shell_name: session.shell_name.clone(),
            cwd: session.cwd.clone(),
        }),
    ))
}

/// Stream one session's output until its shell exits or the client disconnects.
pub async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<SseResponse, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let session = require_session(&state, &session_id, &owner_id)?;
    // A reconnecting client sends back the last frame id it rendered, so the
    // replay buffer repaints only the gap instead of the whole scrollback.
    let resume_after = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let (receiver, replay) = session.subscribe();
    Ok(
        Sse::new(frame_stream(session, receiver, replay, resume_after)).keep_alive(
            KeepAlive::new()
                .interval(KEEP_ALIVE_INTERVAL)
                .text("keep-alive"),
        ),
    )
}

pub async fn input(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<InputRequest>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let session = require_session(&state, &session_id, &owner_id)?;
    if body.data.len() > MAX_INPUT_BYTES {
        return Err(ApiError::invalid_input("terminal input chunk is too large"));
    }

    // The PTY writer blocks when the shell stops draining, so it never runs on
    // a runtime worker.
    tokio::task::spawn_blocking(move || session.write_input(body.data.as_bytes()))
        .await
        .map_err(|_| ApiError::internal("terminal write failed"))?
        .map_err(terminal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn resize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<ResizeRequest>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let session = require_session(&state, &session_id, &owner_id)?;
    let cols = clamp_dimension(body.cols, 80);
    let rows = clamp_dimension(body.rows, 24);
    tokio::task::spawn_blocking(move || session.resize(cols, rows))
        .await
        .map_err(|_| ApiError::internal("terminal resize failed"))?
        .map_err(terminal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn close(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    // Closing an already-gone session is the normal result of a reload racing a
    // cleanup, so it is not an error.
    state.terminals.close(&session_id, &owner_id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn close_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ClosedResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(ClosedResponse {
        closed: state.terminals.close_owned(&owner_id),
    }))
}

fn require_session(
    state: &AppState,
    session_id: &str,
    owner_id: &str,
) -> Result<Arc<TerminalSession>, ApiError> {
    state
        .terminals
        .get(session_id, owner_id)
        .ok_or_else(|| ApiError::not_found("terminal session not found"))
}

fn clamp_dimension(value: u16, fallback: u16) -> u16 {
    if value == 0 {
        fallback
    } else {
        value.min(MAX_DIMENSION)
    }
}

fn terminal_error(error: TerminalError) -> ApiError {
    let status = match error.code {
        "terminal.too_many_sessions" => StatusCode::TOO_MANY_REQUESTS,
        "terminal.shell_spawn_failed" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.code, error.message)
}

/// Resolve the requested working directory and confirm the caller may open a
/// shell there.
///
/// Allowed roots are the account's workspace root and the directory bound to
/// each of its active local workspaces. A terminal therefore reaches exactly as
/// far as the workspace file tools, and no further.
async fn authorize_cwd(
    state: &AppState,
    owner_id: &str,
    requested: &str,
) -> Result<String, ApiError> {
    let requested = requested.trim();
    if requested.is_empty() {
        return Err(ApiError::invalid_input("cwd is required"));
    }
    let canonical = std::fs::canonicalize(requested)
        .map_err(|_| ApiError::invalid_input("cwd must be an existing directory"))?;
    if !canonical.is_dir() {
        return Err(ApiError::invalid_input("cwd must be an existing directory"));
    }

    let mut allowed: Vec<PathBuf> = Vec::new();
    if let Ok(root) = workspaces::resolve_workspace_root(state, owner_id).await {
        allowed.push(root);
    }
    let bound = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT local_path FROM workspaces \
         WHERE owner_id = ? AND status = 'active' AND backend_type = 'local'",
    )
    .bind(owner_id)
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    for (path,) in bound {
        let Some(path) = path else { continue };
        if let Ok(canonical) = std::fs::canonicalize(path) {
            allowed.push(canonical);
        }
    }

    if !allowed.iter().any(|root| canonical.starts_with(root)) {
        return Err(ApiError::permission_denied(
            "cwd is outside this account's workspaces",
        ));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

/// Replay buffered frames, then follow the live channel.
///
/// A frame already replayed can also arrive on the channel, so everything is
/// filtered by sequence number. A subscriber that falls behind is refilled from
/// the session buffer rather than silently losing output.
fn frame_stream(
    session: Arc<TerminalSession>,
    receiver: Receiver<TerminalFrame>,
    replay: Vec<TerminalFrame>,
    resume_after: u64,
) -> BoxStream<'static, Result<Event, Infallible>> {
    struct StreamState {
        session: Arc<TerminalSession>,
        receiver: Receiver<TerminalFrame>,
        pending: VecDeque<TerminalFrame>,
        last_seq: u64,
    }

    stream::unfold(
        Some(StreamState {
            session,
            receiver,
            pending: replay.into(),
            last_seq: resume_after,
        }),
        |state| async move {
            let mut state = state?;
            loop {
                let frame = match state.pending.pop_front() {
                    Some(frame) => frame,
                    None => match state.receiver.recv().await {
                        Ok(frame) => frame,
                        Err(RecvError::Lagged(_)) => {
                            state.pending = state.session.frames_after(state.last_seq).into();
                            continue;
                        }
                        Err(RecvError::Closed) => return None,
                    },
                };
                let (seq, event) = frame;
                if seq <= state.last_seq {
                    continue;
                }
                state.last_seq = seq;
                let finished = matches!(event, TerminalEvent::Exit { .. });
                let payload = serde_json::to_string(&event).unwrap_or_default();
                let sse = Ok(Event::default().id(seq.to_string()).data(payload));
                return Some((sse, if finished { None } else { Some(state) }));
            }
        },
    )
    .boxed()
}
