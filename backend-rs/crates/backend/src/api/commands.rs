//! Durable command acceptance; HTTP and WS share this service and worker.
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use super::{
    auth::{self, AuthenticatedSession},
    error::ApiError,
    messages::{self, MessageInput},
    workspace_files::{self, ConversationRoot, ConversationScope},
    AppState,
};
use crate::runtime::{
    group::{run_thread_resume_with_stream_id, MessageAttachment, ResumeRequest},
    run_group_turn_with_stream_id, RuntimeServices, TurnOutcome, TurnRequest,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Command {
    pub request_id: String,
    pub name: String,
    #[serde(default)]
    pub target: Value,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PreparedMessage {
    pub(crate) content: String,
    pub(crate) attachments: Vec<MessageAttachment>,
    pub(crate) model_override: Option<String>,
    pub(crate) effort_override: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct PreparedResume {
    agent_id: String,
    message_id: String,
    existing_content: String,
    content_json: Option<String>,
    approval: Option<crate::tools::ApprovalDecision>,
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(accept(&state, &owner, command).await?),
    ))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let row:Option<(String,Option<String>,Option<String>)>=sqlx::query_as("SELECT r.response_json,j.status,j.error FROM command_receipts r LEFT JOIN execution_jobs j ON j.owner_id=r.owner_id AND j.id=json_extract(r.response_json,'$.job_id') WHERE r.owner_id=? AND r.request_id=?")
        .bind(owner).bind(request_id).fetch_optional(state.db.pool()).await.map_err(db_error)?;
    let (raw, status, error) = row.ok_or_else(|| ApiError::not_found("command not found"))?;
    let mut value: Value =
        serde_json::from_str(&raw).map_err(|_| ApiError::internal("invalid receipt"))?;
    value["execution_status"] = json!(status);
    value["error"] = json!(error);
    Ok(Json(value))
}

pub async fn ws_command(state: &AppState, session: &AuthenticatedSession, value: Value) -> Value {
    let request_id = value["request_id"].clone();
    match serde_json::from_value::<Command>(value) {
        Ok(command) => match accept(state, &session.owner_id, command).await {
            Ok(result) => json!({"type":"command_result","request_id":request_id,"result":result}),
            Err(error) => {
                json!({"type":"command_result","request_id":request_id,"error":{"code":error.code(),"message":error.message_text(),"details":error.details()}})
            }
        },
        Err(_) => {
            json!({"type":"command_result","request_id":request_id,"error":{"code":"invalid_input"}})
        }
    }
}

pub async fn accept(
    state: &AppState,
    owner: &str,
    mut command: Command,
) -> Result<Value, ApiError> {
    command.request_id = Uuid::parse_str(&command.request_id)
        .map_err(|_| ApiError::invalid_input("request_id must be a UUID"))?
        .to_string();
    if !command.target.is_object() {
        return Err(ApiError::invalid_input("target must be an object"));
    }
    if command.name == "message.send" {
        let mut body: MessageInput = serde_json::from_value(command.payload.clone())
            .map_err(|_| ApiError::invalid_input("invalid message payload"))?;
        let thread = command.target["thread_id"]
            .as_str()
            .map(str::to_string)
            .or(body.thread_id.clone());
        if let (Some(target), Some(payload)) = (
            command.target["thread_id"].as_str(),
            body.thread_id.as_deref(),
        ) {
            if target != payload {
                return Err(ApiError::invalid_input("conflicting thread targets"));
            }
        }
        command.target["thread_id"] = json!(thread);
        body.client_request_id = None;
        body.thread_id = None;
        body.content = body.content.trim().into();
        command.payload = serde_json::to_value(body).unwrap();
    }
    if ![
        "message.send",
        "turn.cancel",
        "turn.acknowledge_stopped",
        "thread.resume",
        "approval.resolve",
    ]
    .contains(&command.name.as_str())
    {
        return Err(ApiError::invalid_input("unsupported command"));
    }
    let digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(
                &json!({"name":command.name,"target":command.target,"payload":command.payload})
            )
            .unwrap()
        )
    );
    // Read before preflight so tombstones still resolve after resource removal.
    if let Some(response) = receipt(state, owner, &command.request_id, &digest).await? {
        return Ok(response);
    }
    match command.name.as_str() {
        "message.send" => accept_message(state, owner, &command, &digest).await,
        "turn.cancel" => accept_cancel(state, owner, &command, &digest).await,
        "turn.acknowledge_stopped" => {
            accept_acknowledge_stopped(state, owner, &command, &digest).await
        }
        _ => accept_resume(state, owner, &command, &digest).await,
    }
}

async fn receipt(
    state: &AppState,
    owner: &str,
    id: &str,
    digest: &str,
) -> Result<Option<Value>, ApiError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT payload_hash,response_json FROM command_receipts WHERE owner_id=? AND request_id=?",
    )
    .bind(owner)
    .bind(id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(db_error)?;
    row.map(|(hash, raw)| decode_receipt(&hash, &raw, digest))
        .transpose()
}

fn decode_receipt(hash: &str, raw: &str, digest: &str) -> Result<Value, ApiError> {
    if hash != digest && hash != "legacy_tombstone" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "request_id was already used with a different command, target or payload",
        ));
    }
    serde_json::from_str(raw).map_err(|_| ApiError::internal("invalid receipt"))
}

async fn existing_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    id: &str,
    digest: &str,
) -> Result<Option<Value>, ApiError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT payload_hash,response_json FROM command_receipts WHERE owner_id=? AND request_id=?",
    )
    .bind(owner)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?;
    row.map(|(hash, raw)| decode_receipt(&hash, &raw, digest))
        .transpose()
}

fn target<'a>(command: &'a Command, key: &str) -> Result<&'a str, ApiError> {
    command.target[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::invalid_input(format!("target.{key} is required")))
}

async fn accept_message(
    state: &AppState,
    owner: &str,
    command: &Command,
    digest: &str,
) -> Result<Value, ApiError> {
    let group = target(command, "conversation_id")?;
    let body: MessageInput = serde_json::from_value(command.payload.clone())
        .map_err(|_| ApiError::invalid_input("invalid message payload"))?;
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT conversation_kind FROM groups WHERE id=? AND owner_id=? AND status='active'",
    )
    .bind(group)
    .bind(owner)
    .fetch_optional(state.db.pool())
    .await
    .map_err(db_error)?;
    let kind = kind.ok_or_else(|| ApiError::not_found("conversation not found"))?;
    if kind == "direct" {
        messages::ensure_direct_agent_available(state.db.pool(), group, owner).await?;
    }
    let paths = body
        .attachments
        .iter()
        .map(|a| a.path.clone())
        .collect::<Vec<_>>();
    let attachments = workspace_files::validate_conversation_attachments(
        state.db.pool(),
        ConversationRoot::conversation(
            if kind == "direct" {
                ConversationScope::DirectChats
            } else {
                ConversationScope::Groups
            },
            group,
            owner,
        ),
        &paths,
    )
    .await?;
    if body.content.trim().is_empty() && attachments.is_empty() {
        return Err(ApiError::invalid_input(
            "content or attachments must not be empty",
        ));
    }
    let model = messages::validate_model_override(
        state.db.pool(),
        group,
        owner,
        body.model_override.as_deref(),
    )
    .await?;
    messages::parse_effort_override(body.effort_override.as_deref())?;
    let prepared = PreparedMessage {
        content: body.content.trim().into(),
        attachments,
        model_override: model,
        effort_override: body.effort_override,
    };
    let now = now();
    let _guard = state.write_lock.lock().await;
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    if let Some(value) = existing_in_tx(&mut tx, owner, &command.request_id, digest).await? {
        return Ok(value);
    }
    let still_owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM groups WHERE id=? AND owner_id=? AND status='active')",
    )
    .bind(group)
    .bind(owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    if !still_owned {
        return Err(ApiError::not_found("conversation not found"));
    }
    let requested = command.target["thread_id"]
        .as_str()
        .or(body.thread_id.as_deref());
    let explicit_waiting = command.target["waiting_job_id"].as_str();
    // Note: a message aimed at a waiting thread is the legacy Continue path.
    // Reuse that job instead of reserving a second turn that the waiting job
    // would block forever. A message with a different explicit thread still
    // queues normally and cannot silently replace waiting work.
    let waiting: Option<(String, String)> = if let Some(job) = explicit_waiting {
        sqlx::query_as("SELECT id,thread_id FROM execution_jobs WHERE id=? AND owner_id=? AND group_id=? AND status='waiting_for_user'")
            .bind(job).bind(owner).bind(group).fetch_optional(&mut *tx).await.map_err(db_error)?
    } else if let Some(thread) = requested {
        sqlx::query_as("SELECT id,thread_id FROM execution_jobs WHERE owner_id=? AND group_id=? AND thread_id=? AND status='waiting_for_user' ORDER BY created_at,id LIMIT 1")
            .bind(owner).bind(group).bind(thread).fetch_optional(&mut *tx).await.map_err(db_error)?
    } else {
        sqlx::query_as("SELECT id,thread_id FROM execution_jobs WHERE owner_id=? AND group_id=? AND status='waiting_for_user' ORDER BY created_at,id LIMIT 1")
            .bind(owner).bind(group).fetch_optional(&mut *tx).await.map_err(db_error)?
    };
    if explicit_waiting.is_some() && waiting.is_none() {
        return Err(ApiError::conflict(
            "waiting job changed; refresh before answering",
        ));
    }
    if let (Some(thread), Some((_, waiting_thread))) = (requested, waiting.as_ref()) {
        if thread != waiting_thread {
            return Err(ApiError::conflict(
                "waiting job changed; refresh before answering",
            ));
        }
    }
    let thread = if let Some((_, waiting_thread)) = &waiting {
        waiting_thread.clone()
    } else if let Some(thread) = requested {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM threads WHERE id=? AND group_id=? AND agent_id IS NULL AND status IN ('active','running','paused'))").bind(thread).bind(group).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !valid{
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM threads WHERE id=? AND group_id=? AND agent_id IS NULL AND status!='cleared')").bind(thread).bind(group).fetch_one(&mut *tx).await.map_err(db_error)?;
            return Err(if exists{ApiError::conflict("thread is not active")}else{ApiError::not_found("thread not found")});
        }
        thread.to_string()
    }else if let Some(thread)=sqlx::query_scalar::<_,String>("SELECT id FROM threads WHERE group_id=? AND agent_id IS NULL AND status='active' ORDER BY updated_at DESC,created_at DESC,id DESC LIMIT 1").bind(group).fetch_optional(&mut *tx).await.map_err(db_error)?{thread}
    else{
        let id=Uuid::new_v4().to_string();
        let title:String=prepared.content.lines().next().unwrap_or("Task").chars().take(80).collect();
        sqlx::query("INSERT INTO threads(id,group_id,agent_id,title,status,next_seq,created_at,updated_at) VALUES(?,?,NULL,?,'active',1,?,?)").bind(&id).bind(group).bind(title).bind(&now).bind(&now).execute(&mut *tx).await.map_err(db_error)?;id
    };
    let reuse_job = waiting.as_ref().map(|(job, _)| job.as_str());
    if reuse_job.is_none() {
        // Note: a new message on a paused task supersedes the interrupted
        // checkpoint from a failed or disconnected turn, matching the legacy
        // send path. A waiting job took the branch below, so this never
        // silently answers an approval gate.
        let superseded = sqlx::query(
            "UPDATE threads SET status='active',updated_at=? WHERE id=? AND status='paused'",
        )
        .bind(&now)
        .bind(&thread)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?
        .rows_affected();
        if superseded == 1 {
            sqlx::query(
                "UPDATE messages SET status='visible' WHERE thread_id=? AND status='interrupted'",
            )
            .bind(&thread)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        }
    }
    if let Some(job) = reuse_job {
        ensure_computer_effects_settled(&mut tx, &thread).await?;
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM execution_jobs WHERE id=? AND owner_id=? AND group_id=? AND thread_id=? AND status='waiting_for_user')")
            .bind(job).bind(owner).bind(group).bind(&thread).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !waiting {
            return Err(ApiError::conflict(
                "waiting job changed; refresh before answering",
            ));
        }
        crate::runtime::group_scheduler::supersede_active_turn_in_tx(&mut tx, &thread, &now)
            .await
            .map_err(|_| ApiError::internal("failed to continue waiting job"))?;
        sqlx::query("UPDATE pending_approvals SET status='cancelled',version=version+1,updated_at=? WHERE thread_id=? AND status='pending'").bind(&now).bind(&thread).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query(
            "UPDATE messages SET status='visible' WHERE thread_id=? AND status='interrupted'",
        )
        .bind(&thread)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("UPDATE threads SET status='active',updated_at=? WHERE id=?")
            .bind(&now)
            .bind(&thread)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
    }
    let response = insert_message_in_tx(
        &mut tx,
        MessageAcceptance {
            owner,
            command,
            digest,
            group,
            thread: &thread,
            prepared: &prepared,
            now: &now,
            reuse_job,
        },
    )
    .await?;
    tx.commit().await.map_err(db_error)?;
    drop(_guard);
    launch(state.clone());
    Ok(response)
}

pub(crate) struct MessageAcceptance<'a> {
    pub owner: &'a str,
    pub command: &'a Command,
    pub digest: &'a str,
    pub group: &'a str,
    pub thread: &'a str,
    pub prepared: &'a PreparedMessage,
    pub now: &'a str,
    pub reuse_job: Option<&'a str>,
}

/// Shared transaction core: timer/webhook acceptance also writes the message,
/// reserved turn, job and receipt before acknowledging any occurrence.
pub(crate) async fn insert_message_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: MessageAcceptance<'_>,
) -> Result<Value, ApiError> {
    let MessageAcceptance {
        owner,
        command,
        digest,
        group,
        thread,
        prepared,
        now,
        reuse_job,
    } = input;
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM group_turns WHERE thread_id=? AND status IN ('pending','running','waiting_for_user'))")
        .bind(thread).fetch_one(&mut **tx).await.map_err(db_error)?;
    // Note: queued turns sit outside the existing per-thread active index. A
    // claimant promotes exactly one, preserving atomic acceptance and history.
    let turn_status = if busy { "queued" } else { "pending" };
    let job = reuse_job
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let turn = Uuid::new_v4().to_string();
    let message = Uuid::new_v4().to_string();
    // The legacy client_request_id is the same durable stream key, never an
    // independent deduplication path.
    let stream_used:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM execution_jobs WHERE stream_id=?) OR EXISTS(SELECT 1 FROM stream_events WHERE stream_id=?)")
        .bind(&command.request_id).bind(&command.request_id).fetch_one(&mut **tx).await.map_err(db_error)?;
    let stream = if stream_used {
        Uuid::new_v4().to_string()
    } else {
        command.request_id.clone()
    };
    let seq: i64 = sqlx::query_scalar(
        "UPDATE threads SET next_seq=next_seq+1,updated_at=? WHERE id=? RETURNING next_seq-1",
    )
    .bind(now)
    .bind(thread)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    // Note: accepting input makes it visible to clients, but only the claimant
    // may admit it into model history after the preceding worker has stopped.
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,sender_id,message_type,content,content_json,status,created_at,context_admitted) VALUES(?,?,?,?,'user',?,'text',?,?,'visible',?,0)")
        .bind(&message).bind(thread).bind(group).bind(seq).bind(owner).bind(&prepared.content).bind(crate::runtime::group::user_attachment_content_json(&prepared.attachments)).bind(now).execute(&mut **tx).await.map_err(db_error)?;
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,trigger_message_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,?,?,'bounded','{}','{}',?,?)")
        .bind(&turn).bind(thread).bind(group).bind(&message).bind(turn_status).bind(now).bind(now).execute(&mut **tx).await.map_err(db_error)?;
    sqlx::query("UPDATE messages SET turn_id=? WHERE id=?")
        .bind(&turn)
        .bind(&message)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    let payload = json!({"message_id":message,"thread_id":thread,"content":prepared.content,"attachments":prepared.attachments,"sender_type":"user"});
    sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,0,?,'user_message',?,?)")
        .bind(Uuid::new_v4().to_string()).bind(&stream).bind(thread).bind(format!("{stream}:0")).bind(payload.to_string()).bind(now).execute(&mut **tx).await.map_err(db_error)?;
    let response = json!({"request_id":command.request_id,"status":"accepted","job_id":job,"turn_id":turn,"stream_id":stream,"thread_id":thread});
    if reuse_job.is_some() {
        sqlx::query("UPDATE execution_jobs SET command_name='message.send',thread_id=?,turn_id=?,stream_id=?,message_id=?,payload_json=?,status='queued',error=NULL,updated_at=? WHERE id=?")
            .bind(thread).bind(&turn).bind(&stream).bind(&message).bind(serde_json::to_string(prepared).unwrap()).bind(now).bind(&job).execute(&mut **tx).await.map_err(db_error)?;
    } else {
        insert_job(
            tx,
            owner,
            command,
            &response,
            group,
            thread,
            &turn,
            &stream,
            Some(&message),
            serde_json::to_string(&prepared).unwrap(),
            now,
        )
        .await?;
    }
    insert_receipt(tx, owner, command, digest, &response, now).await?;
    Ok(response)
}

async fn insert_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    command: &Command,
    digest: &str,
    response: &Value,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO command_receipts(owner_id,request_id,command_name,payload_hash,response_json,created_at) VALUES(?,?,?,?,?,?)")
        .bind(owner).bind(&command.request_id).bind(&command.name).bind(digest).bind(response.to_string()).bind(now).execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_job(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    command: &Command,
    response: &Value,
    group: &str,
    thread: &str,
    turn: &str,
    stream: &str,
    message: Option<&str>,
    payload: String,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,message_id,payload_json,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,'queued',?,?)")
        .bind(response["job_id"].as_str()).bind(owner).bind(&command.request_id).bind(&command.name).bind(group).bind(thread).bind(turn).bind(stream).bind(message).bind(payload).bind(now).bind(now)
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

/// Write the same terminal pair a cancelled run leaves behind. `done` is
/// guarded, so replaying a recovery path cannot append a second terminal.
async fn append_stream_terminal(
    tx: &mut Transaction<'_, Sqlite>,
    stream: &str,
    thread: &str,
    turn: &str,
    reason: &str,
    now: &str,
) -> Result<(), sqlx::Error> {
    for (kind, payload) in [
        ("turn_cancelled", json!({"turn_id":turn,"reason":reason})),
        ("done", json!({})),
    ] {
        sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) SELECT ?,?,?,COALESCE(MAX(seq)+1,0),? || ':' || CAST(COALESCE(MAX(seq)+1,0) AS TEXT),?,?,? FROM stream_events WHERE stream_id=? HAVING NOT EXISTS(SELECT 1 FROM stream_events WHERE stream_id=? AND kind='done')")
            .bind(Uuid::new_v4().to_string()).bind(stream).bind(thread).bind(stream).bind(kind).bind(payload.to_string()).bind(now).bind(stream).bind(stream).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Cancel an automation job the worker loop has not claimed yet. Timer
/// coalescing and automation edits use this path so the reserved turn and its
/// stream end in the same transaction. A claimed worker is never touched:
/// cancellation cannot hand the group to another worker before the old one is
/// confirmed stopped.
pub(crate) async fn cancel_queued_job_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    job_id: &str,
) -> Result<(), sqlx::Error> {
    let job: Option<(String, String, String)> = sqlx::query_as(
        "SELECT turn_id,stream_id,thread_id FROM execution_jobs WHERE id=? AND status='queued' AND worker_active=0",
    )
    .bind(job_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((turn, stream, thread)) = job else {
        return Ok(());
    };
    let now = now();
    // An unclaimed job can sit behind a busy group with its turn still
    // `pending` (the thread itself was idle), so both pre-running states are
    // cancellable here. A claimed job already moved to `running` and never
    // reaches this path.
    let changed = sqlx::query(
        "UPDATE group_turns SET status='cancelled',termination_reason='superseded',completed_at=?,updated_at=? WHERE id=? AND status IN ('queued','pending')",
    )
    .bind(&now)
    .bind(&now)
    .bind(&turn)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Ok(());
    }
    sqlx::query("UPDATE execution_jobs SET status='cancelled',occupies_group=0,worker_active=0,updated_at=? WHERE id=? AND status='queued'")
        .bind(&now)
        .bind(job_id)
        .execute(&mut **tx)
        .await?;
    append_stream_terminal(tx, &stream, &thread, &turn, "superseded", &now).await?;
    Ok(())
}

async fn accept_cancel(
    state: &AppState,
    owner: &str,
    command: &Command,
    digest: &str,
) -> Result<Value, ApiError> {
    let turn = target(command, "turn_id")?;
    let _guard = state.write_lock.lock().await;
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    if let Some(value) = existing_in_tx(&mut tx, owner, &command.request_id, digest).await? {
        return Ok(value);
    }
    let thread:Option<String>=sqlx::query_scalar("SELECT r.thread_id FROM group_turns r JOIN groups g ON g.id=r.group_id WHERE r.id=? AND g.owner_id=? AND g.status='active'").bind(turn).bind(owner).fetch_optional(&mut *tx).await.map_err(db_error)?;
    let thread = thread.ok_or_else(|| ApiError::not_found("turn not found"))?;
    let queued: Vec<(String, String)> = sqlx::query_as(
        "SELECT stream_id,thread_id FROM execution_jobs WHERE turn_id=? AND status='queued'",
    )
    .bind(turn)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    crate::runtime::group_scheduler::store::cancel_turn_in_tx(&mut tx, turn, &now())
        .await
        .map_err(|_| ApiError::internal("failed to cancel turn"))?;
    sqlx::query("UPDATE execution_jobs SET status='cancelled',occupies_group=CASE WHEN worker_active=0 THEN 0 ELSE occupies_group END,updated_at=? WHERE turn_id=? AND status IN ('queued','running','waiting_for_user')").bind(now()).bind(turn).execute(&mut *tx).await.map_err(db_error)?;
    // Note: a thread can already contain a newer approval when an old turn's
    // cancel is delivered. The checkpoint identifies the question's own turn.
    sqlx::query("UPDATE pending_approvals SET status='cancelled',version=version+1,updated_at=? WHERE thread_id=? AND status='pending' AND EXISTS(SELECT 1 FROM messages m,json_each(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '{}' END,'$.tool_calls') c WHERE m.thread_id=pending_approvals.thread_id AND m.turn_id=? AND json_extract(c.value,'$.tool_call_id')=pending_approvals.tool_call_id)")
        .bind(now()).bind(&thread).bind(turn).execute(&mut *tx).await.map_err(db_error)?;
    for (stream, thread) in queued {
        append_stream_terminal(&mut tx, &stream, &thread, turn, "user_cancelled", &now())
            .await
            .map_err(db_error)?;
    }
    let response = json!({"request_id":command.request_id,"status":"accepted","turn_id":turn});
    insert_receipt(&mut tx, owner, command, digest, &response, &now()).await?;
    tx.commit().await.map_err(db_error)?;
    drop(_guard);
    state.active_turns.cancel(&thread, turn).await;
    launch(state.clone());
    Ok(response)
}

/// Release a group whose job was interrupted by a restart. The command records
/// an explicit human confirmation that the old worker and its external effects
/// are stopped; it never retries the interrupted action itself. Claiming keeps
/// treating every unconfirmed interrupted job as group-blocking.
async fn accept_acknowledge_stopped(
    state: &AppState,
    owner: &str,
    command: &Command,
    digest: &str,
) -> Result<Value, ApiError> {
    let turn = target(command, "turn_id")?;
    let _guard = state.write_lock.lock().await;
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    if let Some(value) = existing_in_tx(&mut tx, owner, &command.request_id, digest).await? {
        return Ok(value);
    }
    let owned_turn: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution_jobs j JOIN groups g ON g.id=j.group_id \
         WHERE j.turn_id=? AND g.owner_id=? AND g.status='active')",
    )
    .bind(turn)
    .bind(owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    if !owned_turn {
        return Err(ApiError::not_found("turn not found"));
    }
    let jobs: Vec<String> = sqlx::query_scalar(
        "UPDATE execution_jobs \
         SET error='effects_verified_stopped',occupies_group=0,worker_active=0,updated_at=? \
         WHERE turn_id=? AND status='interrupted' \
           AND COALESCE(error,'')!='effects_verified_stopped' \
         RETURNING id",
    )
    .bind(now())
    .bind(turn)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    if jobs.is_empty() {
        return Err(ApiError::conflict(
            "no interrupted job is waiting for stop confirmation",
        ));
    }
    sqlx::query("UPDATE computer_operations SET status='acknowledged' WHERE turn_id=? AND status='uncertain'").bind(turn).execute(&mut *tx).await.map_err(db_error)?;
    let response =
        json!({"request_id":command.request_id,"status":"accepted","turn_id":turn,"jobs":jobs});
    insert_receipt(&mut tx, owner, command, digest, &response, &now()).await?;
    tx.commit().await.map_err(db_error)?;
    drop(_guard);
    launch(state.clone());
    Ok(response)
}

async fn accept_resume(
    state: &AppState,
    owner: &str,
    command: &Command,
    digest: &str,
) -> Result<Value, ApiError> {
    let thread = target(command, "thread_id")?;
    let target = match super::threads::resolve_resume_target(state.db.pool(), thread, owner).await {
        Ok(target) => target,
        Err(error) if command.name == "approval.resolve" && error.code() == "conflict" => {
            let row:Option<(String,i64,Option<String>)>=sqlx::query_as("SELECT status,version,decision_json FROM pending_approvals WHERE owner_id=? AND thread_id=? AND tool_call_id=?")
                .bind(owner).bind(thread).bind(command.payload["tool_call_id"].as_str().unwrap_or("")).fetch_optional(state.db.pool()).await.map_err(db_error)?;
            return Err(ApiError::new(StatusCode::CONFLICT,"approval_conflict",error.message_text()).with_details(json!({"current":row.map(|(status,version,decision)|json!({"status":status,"version":version,"decision":decision.and_then(|s|serde_json::from_str::<Value>(&s).ok())}))})));
        }
        Err(error) => return Err(error),
    };
    crate::runtime::approval::ensure_pending(
        state.db.pool(),
        thread,
        target.content_json.as_deref(),
    )
    .await
    .map_err(|_| ApiError::conflict("invalid pending approval checkpoint"))?;
    let decision = if command.name == "approval.resolve" {
        Some(
            serde_json::from_value::<crate::tools::ApprovalDecision>(command.payload.clone())
                .map_err(|_| ApiError::invalid_input("invalid approval decision"))?,
        )
    } else {
        None
    };
    let pending:Option<(String,String,i64,String)>=sqlx::query_as("SELECT id,action_hash,version,action_json FROM pending_approvals WHERE thread_id=? AND status='pending' ORDER BY created_at DESC LIMIT 1").bind(thread).fetch_optional(state.db.pool()).await.map_err(db_error)?;
    let prepared = PreparedResume {
        agent_id: target.agent_id,
        message_id: target.message_id,
        existing_content: target.existing_content,
        content_json: target.content_json,
        approval: decision.clone(),
    };
    let _guard = state.write_lock.lock().await;
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    if let Some(value) = existing_in_tx(&mut tx, owner, &command.request_id, digest).await? {
        return Ok(value);
    }
    ensure_computer_effects_settled(&mut tx, thread).await?;
    let current_content: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT content_json FROM messages WHERE id=? AND thread_id=? AND status='interrupted'",
    )
    .bind(&prepared.message_id)
    .bind(thread)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    if current_content.as_ref().map(|row| &row.0) != Some(&prepared.content_json) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "approval_conflict",
            "approval checkpoint changed",
        ));
    }
    if decision.is_none() {
        // Preserve the existing explicit Continue action: it skips an unanswered
        // gated call, never grants it, and lets the model choose another action.
        sqlx::query("UPDATE pending_approvals SET status='cancelled',version=version+1,updated_at=? WHERE thread_id=? AND status='pending'").bind(now()).bind(thread).execute(&mut *tx).await.map_err(db_error)?;
    }
    if let Some((id, hash, version, action)) = pending.filter(|_| decision.is_some()) {
        let decision = decision
            .as_ref()
            .ok_or_else(|| ApiError::conflict("approval required"))?;
        let action: Value = serde_json::from_str(&action)
            .map_err(|_| ApiError::internal("invalid approval action"))?;
        if command.payload["expected_version"].as_i64() != Some(version)
            || command.payload["action_hash"].as_str() != Some(&hash)
            || action["id"] != decision.tool_call_id
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "approval_conflict",
                "approval version or action does not match",
            ));
        }
        let changed=sqlx::query("UPDATE pending_approvals SET status='resolved',version=version+1,decision_json=?,updated_at=? WHERE id=? AND status='pending' AND version=?")
            .bind(serde_json::to_string(decision).unwrap()).bind(now()).bind(id).bind(version).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if changed != 1 {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "approval_conflict",
                "approval was already resolved",
            ));
        }
    } else if decision.is_some() {
        return Err(ApiError::conflict("no matching pending approval"));
    }
    let changed = sqlx::query(
        "UPDATE threads SET status='running',updated_at=? WHERE id=? AND status='paused'",
    )
    .bind(now())
    .bind(thread)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?
    .rows_affected();
    if changed != 1 {
        return Err(ApiError::conflict("thread is already resuming"));
    }
    let turn:Option<String>=sqlx::query_scalar("SELECT id FROM group_turns WHERE thread_id=? AND status='waiting_for_user' ORDER BY created_at DESC LIMIT 1").bind(thread).fetch_optional(&mut *tx).await.map_err(db_error)?;
    let turn = if let Some(turn) = turn {
        turn
    } else {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,'running','bounded','{}','{}',?,?)")
            .bind(&id).bind(thread).bind(&target.group_id).bind(now()).bind(now()).execute(&mut *tx).await.map_err(db_error)?;
        id
    };
    sqlx::query("UPDATE group_turns SET status='running',termination_reason=NULL,updated_at=? WHERE id=? AND status='waiting_for_user'").bind(now()).bind(&turn).execute(&mut *tx).await.map_err(db_error)?;
    let stream = command.request_id.clone();
    let existing_job:Option<String>=sqlx::query_scalar("SELECT id FROM execution_jobs WHERE owner_id=? AND turn_id=? AND status IN ('running','waiting_for_user') ORDER BY created_at LIMIT 1")
        .bind(owner).bind(&turn).fetch_optional(&mut *tx).await.map_err(db_error)?;
    let job_id = existing_job
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let response = json!({"request_id":command.request_id,"status":"accepted","job_id":job_id,"turn_id":turn,"stream_id":stream,"thread_id":thread});
    let prepared_json = serde_json::to_string(&prepared).unwrap();
    if existing_job.is_some() {
        sqlx::query("UPDATE execution_jobs SET command_name=?,stream_id=?,payload_json=?,message_id=NULL,status='queued',error=NULL,updated_at=? WHERE id=?")
            .bind(&command.name).bind(&stream).bind(prepared_json).bind(now()).bind(job_id).execute(&mut *tx).await.map_err(db_error)?;
    } else {
        insert_job(
            &mut tx,
            owner,
            command,
            &response,
            &target.group_id,
            thread,
            &turn,
            &stream,
            None,
            prepared_json,
            &now(),
        )
        .await?;
    }
    insert_receipt(&mut tx, owner, command, digest, &response, &now()).await?;
    tx.commit().await.map_err(db_error)?;
    drop(_guard);
    launch(state.clone());
    Ok(response)
}

pub fn launch(state: AppState) {
    tokio::spawn(async move {
        if let Err(error) = claim_and_launch(&state).await {
            tracing::error!(%error,"failed to claim durable jobs");
        }
    });
}

#[derive(sqlx::FromRow)]
struct Job {
    id: String,
    owner_id: String,
    group_id: String,
    thread_id: String,
    turn_id: String,
    stream_id: String,
    command_name: String,
    payload_json: String,
    generation: i64,
}

/// Atomically claim at most one queued job. Group occupancy is decided here:
/// a waiting or interrupted-unconfirmed job blocks every other job in the same
/// group, and manual jobs sort ahead of not-yet-started automatic work.
async fn claim_next(state: &AppState) -> Result<Option<Job>, sqlx::Error> {
    let mut tx = state.db.begin_write().await?;
    sqlx::query("UPDATE execution_jobs SET occupies_group=1 WHERE id IN (SELECT w.id FROM execution_jobs w WHERE w.status='waiting_for_user' AND w.occupies_group=0 AND NOT EXISTS(SELECT 1 FROM execution_jobs a WHERE a.owner_id=w.owner_id AND a.group_id=w.group_id AND a.occupies_group=1) AND w.id=(SELECT f.id FROM execution_jobs f WHERE f.owner_id=w.owner_id AND f.group_id=w.group_id AND f.status='waiting_for_user' ORDER BY f.created_at,f.id LIMIT 1))").execute(&mut *tx).await?;
    let row:Option<Job>=sqlx::query_as("UPDATE execution_jobs SET status='running',occupies_group=1,worker_active=1,generation=generation+1,updated_at=? WHERE id=(SELECT q.id FROM execution_jobs q WHERE q.status='queued' AND q.worker_active=0 AND NOT EXISTS(SELECT 1 FROM execution_jobs a WHERE a.owner_id=q.owner_id AND a.group_id=q.group_id AND a.id!=q.id AND (a.occupies_group=1 OR (a.status='interrupted' AND COALESCE(a.error,'')!='effects_verified_stopped'))) AND NOT EXISTS(SELECT 1 FROM group_turns t WHERE t.thread_id=q.thread_id AND t.id!=q.turn_id AND t.status IN ('pending','running','waiting_for_user')) ORDER BY q.occupies_group DESC,(q.automation_id IS NULL) DESC,q.created_at,q.id LIMIT 1) AND status='queued' RETURNING id,owner_id,group_id,thread_id,turn_id,stream_id,command_name,payload_json,generation").bind(now()).fetch_optional(&mut *tx).await?;
    if let Some(job) = &row {
        // Note: a previous worker may have appended replies or compacted past
        // the acceptance seq. Allocate history order only after owning the group.
        let unadmitted: Option<(String, i64)> = sqlx::query_as(
            "SELECT m.id,m.seq FROM messages m JOIN execution_jobs j ON j.message_id=m.id WHERE j.id=? AND m.context_admitted=0",
        )
        .bind(&job.id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((message, accepted_seq)) = unadmitted {
            let overtaken: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=? AND context_admitted=1 AND seq>=?) OR EXISTS(SELECT 1 FROM context_checkpoints WHERE thread_id=? AND through_seq>=?)",
            ).bind(&job.thread_id).bind(accepted_seq).bind(&job.thread_id).bind(accepted_seq).fetch_one(&mut *tx).await?;
            let seq: i64 = if overtaken {
                sqlx::query_scalar(
                    "UPDATE threads SET next_seq=next_seq+1 WHERE id=? RETURNING next_seq-1",
                )
                .bind(&job.thread_id)
                .fetch_one(&mut *tx)
                .await?
            } else {
                accepted_seq
            };
            sqlx::query("UPDATE messages SET seq=?,context_admitted=1 WHERE id=?")
                .bind(seq)
                .bind(message)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "UPDATE group_turns SET status='pending',updated_at=? WHERE id=? AND status='queued'",
        )
        .bind(now())
        .bind(&job.turn_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(row)
}

async fn claim_and_launch(state: &AppState) -> Result<(), sqlx::Error> {
    while let Some(job) = claim_next(state).await? {
        let state = state.clone();
        tokio::spawn(async move {
            run_job(state, job).await;
        });
    }
    Ok(())
}

async fn run_job(state: AppState, job: Job) {
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone())
        .with_computers(state.computers.clone())
        .with_active_turn_registry(state.active_turns.clone());
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let stream = Uuid::parse_str(&job.stream_id).unwrap();
    let run = async {
        let runnable:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM groups g JOIN threads t ON t.group_id=g.id WHERE g.id=? AND g.owner_id=? AND g.status='active' AND t.id=? AND t.status!='cleared') AND EXISTS(SELECT 1 FROM group_turns WHERE id=? AND status IN ('pending','running'))")
            .bind(&job.group_id).bind(&job.owner_id).bind(&job.thread_id).bind(&job.turn_id).fetch_one(state.db.pool()).await.unwrap_or(false);
        if !runnable {
            return TurnOutcome::Cancelled;
        }
        if job.command_name == "message.send" {
            let Ok(payload) = serde_json::from_str::<PreparedMessage>(&job.payload_json) else {
                return TurnOutcome::Error;
            };
            run_group_turn_with_stream_id(
                services,
                TurnRequest {
                    group_id: job.group_id.clone(),
                    owner_id: job.owner_id.clone(),
                    thread_id: Some(job.thread_id.clone()),
                    content: payload.content,
                    attachments: payload.attachments,
                    model_override: payload.model_override,
                    effort_override: messages::parse_effort_override(
                        payload.effort_override.as_deref(),
                    )
                    .unwrap_or(None),
                },
                tx,
                stream,
            )
            .await
        } else {
            let Ok(payload) = serde_json::from_str::<PreparedResume>(&job.payload_json) else {
                return TurnOutcome::Error;
            };
            run_thread_resume_with_stream_id(
                services,
                ResumeRequest {
                    group_id: job.group_id.clone(),
                    thread_id: job.thread_id.clone(),
                    agent_id: payload.agent_id,
                    message_id: payload.message_id,
                    existing_content: payload.existing_content,
                    content_json: payload.content_json,
                    approval: payload.approval,
                },
                tx,
                stream,
            )
            .await
        }
    };
    // Note: automation budgets cap model work. The runtime reads the step and
    // token ceilings from the job snapshot; a timer only signals cancellation at
    // the next safe checkpoint instead of dropping a future mid-write.
    let timeout_seconds: Option<u64> = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT json_extract(automation_snapshot_json,'$.budget.turn_timeout_seconds') \
         FROM execution_jobs WHERE id=?",
    )
    .bind(&job.id)
    .fetch_optional(state.db.pool())
    .await
    .ok()
    .flatten()
    .flatten()
    .map(|seconds| seconds.clamp(1, 3600) as u64);
    let timer = timeout_seconds.map(|seconds| {
        let registry = state.active_turns.clone();
        let thread = job.thread_id.clone();
        let turn = job.turn_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
            registry.cancel(&thread, &turn).await;
        })
    });
    // Note: draining is owned by the server, so no client or slow subscription
    // can apply channel backpressure to model/tool execution.
    let (outcome, _) = tokio::join!(run, async { while rx.recv().await.is_some() {} });
    if let Some(timer) = timer {
        timer.abort();
    }
    if let Err(error) = finish_job(&state, &job, outcome).await {
        tracing::error!(job_id=job.id,%error,"failed to finalize durable job");
    }
}

async fn finish_job(state: &AppState, job: &Job, outcome: TurnOutcome) -> Result<(), sqlx::Error> {
    let mut tx = loop {
        let mut tx = state.db.begin_write().await?;
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM computer_operations WHERE turn_id=? AND status='running')",
        )
        .bind(&job.turn_id)
        .fetch_one(&mut *tx)
        .await?;
        if !pending {
            break tx;
        }
        tx.rollback().await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    let current: Option<(i64, String, String)> =
        sqlx::query_as("SELECT generation,stream_id,status FROM execution_jobs WHERE id=?")
            .bind(&job.id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((generation, stream, status)) = current else {
        return Ok(());
    };
    // A continuation can be accepted as the previous stream is ending. Its
    // generation and stream fence out this worker's late finalization.
    if generation != job.generation {
        return Ok(());
    }
    if stream != job.stream_id {
        sqlx::query(
            "UPDATE execution_jobs SET worker_active=0,updated_at=? WHERE id=? AND generation=?",
        )
        .bind(now())
        .bind(&job.id)
        .bind(job.generation)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        launch(state.clone());
        return Ok(());
    }
    let uncertain: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM computer_operations WHERE turn_id=? AND status='uncertain')",
    )
    .bind(&job.turn_id)
    .fetch_one(&mut *tx)
    .await?;
    if uncertain {
        sqlx::query("UPDATE execution_jobs SET status='interrupted',error='computer_execution_uncertain',worker_active=0,occupies_group=1,updated_at=? WHERE id=? AND generation=?").bind(now()).bind(&job.id).bind(job.generation).execute(&mut *tx).await?;
        sqlx::query("UPDATE group_turns SET status='failed',updated_at=? WHERE id=? AND status IN ('pending','running')").bind(now()).bind(&job.turn_id).execute(&mut *tx).await?;
        append_stream_terminal(
            &mut tx,
            &job.stream_id,
            &job.thread_id,
            &job.turn_id,
            "computer_execution_uncertain",
            &now(),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) SELECT ?,?, ?,COALESCE(MAX(seq)+1,0),? || ':' || CAST(COALESCE(MAX(seq)+1,0) AS TEXT),'done','{}',? FROM stream_events WHERE stream_id=? HAVING NOT EXISTS(SELECT 1 FROM stream_events WHERE stream_id=? AND kind='done')")
        .bind(Uuid::new_v4().to_string()).bind(&job.stream_id).bind(&job.thread_id).bind(&job.stream_id).bind(now()).bind(&job.stream_id).bind(&job.stream_id).execute(&mut *tx).await?;
    if status == "running" {
        let (job_status, turn_status) = match outcome {
            TurnOutcome::Completed | TurnOutcome::Silence => ("succeeded", "completed"),
            TurnOutcome::WaitingForUser => ("waiting_for_user", "waiting_for_user"),
            TurnOutcome::Cancelled => ("cancelled", "cancelled"),
            TurnOutcome::Error => ("failed", "failed"),
        };
        sqlx::query("UPDATE execution_jobs SET status=?,updated_at=? WHERE id=? AND generation=? AND status='running'").bind(job_status).bind(now()).bind(&job.id).bind(job.generation).execute(&mut *tx).await?;
        sqlx::query("UPDATE group_turns SET status=?,updated_at=? WHERE id=? AND status IN ('pending','running')").bind(turn_status).bind(now()).bind(&job.turn_id).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE execution_jobs SET worker_active=0,occupies_group=CASE WHEN status='waiting_for_user' THEN 1 ELSE 0 END WHERE id=? AND generation=?")
        .bind(&job.id).bind(job.generation).execute(&mut *tx).await?;
    tx.commit().await?;
    launch(state.clone());
    Ok(())
}

pub async fn recover(state: &AppState) -> Result<(), sqlx::Error> {
    let mut tx = state.db.begin_write().await?;
    sqlx::query("UPDATE computer_operations SET status='uncertain' WHERE status='running'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE execution_jobs SET status=CASE WHEN EXISTS(SELECT 1 FROM group_turns r WHERE r.id=execution_jobs.turn_id AND r.status='waiting_for_user') THEN 'waiting_for_user' WHEN EXISTS(SELECT 1 FROM group_turns r WHERE r.id=execution_jobs.turn_id AND r.status='cancelled') THEN 'cancelled' WHEN EXISTS(SELECT 1 FROM group_turns r WHERE r.id=execution_jobs.turn_id AND r.status IN ('completed','silence','budget_exhausted','failure_budget_exhausted') AND EXISTS(SELECT 1 FROM stream_events e WHERE e.stream_id=execution_jobs.stream_id AND e.kind='done')) THEN 'succeeded' ELSE 'interrupted' END,error='server_restart',updated_at=? WHERE status='running'")
        .bind(now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE execution_jobs SET status='interrupted',error='server_restart',updated_at=? WHERE status IN ('queued','cancelled') AND worker_active=1").bind(now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE execution_jobs SET status='interrupted',error='computer_execution_uncertain',occupies_group=1,updated_at=? WHERE turn_id IN (SELECT turn_id FROM computer_operations WHERE status='uncertain')").bind(now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE execution_jobs SET worker_active=0,occupies_group=CASE WHEN status IN ('succeeded','failed','cancelled') THEN 0 ELSE occupies_group END").execute(&mut *tx).await?;
    sqlx::query("UPDATE execution_jobs SET error=NULL WHERE error='server_restart' AND status IN ('waiting_for_user','succeeded','cancelled')").execute(&mut *tx).await?;
    let interrupted:Vec<(String,String)>=sqlx::query_as("SELECT stream_id,thread_id FROM execution_jobs WHERE status='interrupted' AND error IN ('server_restart','computer_execution_uncertain')")
        .fetch_all(&mut *tx).await?;
    for (stream, thread) in interrupted {
        for (kind, payload) in [
            (
                "error",
                json!({"code":"server_restart","message":"Execution was interrupted by server restart; verify external effects before continuing"}),
            ),
            ("done", json!({})),
        ] {
            sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) SELECT ?,?,?,COALESCE(MAX(seq)+1,0),? || ':' || CAST(COALESCE(MAX(seq)+1,0) AS TEXT),?,?,? FROM stream_events WHERE stream_id=? HAVING NOT EXISTS(SELECT 1 FROM stream_events WHERE stream_id=? AND kind='done')")
                .bind(Uuid::new_v4().to_string()).bind(&stream).bind(&thread).bind(&stream).bind(kind).bind(payload.to_string()).bind(now()).bind(&stream).bind(&stream).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    let checkpoints: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT thread_id,content_json FROM messages WHERE status='interrupted'")
            .fetch_all(state.db.pool())
            .await?;
    for (thread, content) in checkpoints {
        if let Err(error) =
            crate::runtime::approval::ensure_pending(state.db.pool(), &thread, content.as_deref())
                .await
        {
            tracing::warn!(%error,thread_id=thread,"invalid legacy approval checkpoint");
        }
    }
    Ok(())
}

pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
fn db_error(_: sqlx::Error) -> ApiError {
    ApiError::internal("database error")
}

async fn ensure_computer_effects_settled(
    tx: &mut Transaction<'_, Sqlite>,
    thread: &str,
) -> Result<(), ApiError> {
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computer_operations o JOIN group_turns t ON t.id=o.turn_id WHERE t.thread_id=? AND o.status IN ('running','uncertain'))").bind(thread).fetch_one(&mut **tx).await.map_err(db_error)?;
    if pending {
        return Err(ApiError::conflict(
            "Computer action must finish or be explicitly verified stopped before continuing",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod claim_tests {
    use super::{claim_next, AppState};

    const NOW: &str = "2026-10-18T12:00:00Z";

    async fn state() -> AppState {
        let (_router, state) = crate::api::router_with_state_for_tests().await;
        sqlx::query(
            "INSERT INTO users (id,email,password_hash,name,created_at,updated_at) \
             VALUES ('owner-1','claim@example.test','hash','Owner',?,?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(state.db.pool())
        .await
        .unwrap();
        state
    }

    #[allow(clippy::too_many_arguments)]
    async fn job(
        state: &AppState,
        id: &str,
        thread: &str,
        created: &str,
        automation: Option<&str>,
        status: &str,
        error: Option<&str>,
        occupies: i64,
        worker: i64,
    ) {
        sqlx::query(
            "INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,error,occupies_group,worker_active,automation_id,created_at,updated_at) \
             VALUES(?,? ,?,'message.send','group-1',?,?,?,'{}',?,?,?,?,?,?,?)",
        )
        .bind(id)
        .bind("owner-1")
        .bind(format!("request-{id}"))
        .bind(thread)
        .bind(format!("turn-{id}"))
        .bind(format!("stream-{id}"))
        .bind(status)
        .bind(error)
        .bind(occupies)
        .bind(worker)
        .bind(automation)
        .bind(created)
        .bind(NOW)
        .execute(state.db.pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn manual_jobs_claim_before_older_automatic_jobs_and_group_is_exclusive() {
        let state = state().await;
        job(
            &state,
            "auto",
            "thread-auto",
            "2026-10-18T11:00:00Z",
            Some("automation-1"),
            "queued",
            None,
            0,
            0,
        )
        .await;
        job(
            &state,
            "manual",
            "thread-manual",
            "2026-10-18T11:59:00Z",
            None,
            "queued",
            None,
            0,
            0,
        )
        .await;

        let claimed = claim_next(&state).await.unwrap().unwrap();
        assert_eq!(claimed.id, "manual");
        // The claimed job now owns the group; newer work waits even if older.
        assert!(claim_next(&state).await.unwrap().is_none());

        sqlx::query(
            "UPDATE execution_jobs SET status='succeeded',occupies_group=0 WHERE id='manual'",
        )
        .execute(state.db.pool())
        .await
        .unwrap();
        let claimed = claim_next(&state).await.unwrap().unwrap();
        assert_eq!(claimed.id, "auto");
    }

    #[tokio::test]
    async fn unconfirmed_interrupted_job_blocks_the_group_until_stopped() {
        let state = state().await;
        job(
            &state,
            "crashed",
            "thread-crashed",
            "2026-10-18T11:00:00Z",
            None,
            "interrupted",
            Some("server_restart"),
            1,
            0,
        )
        .await;
        job(
            &state,
            "next",
            "thread-next",
            "2026-10-18T11:01:00Z",
            None,
            "queued",
            None,
            0,
            0,
        )
        .await;
        assert!(claim_next(&state).await.unwrap().is_none());

        sqlx::query("UPDATE execution_jobs SET error='effects_verified_stopped',occupies_group=0 WHERE id='crashed'")
            .execute(state.db.pool())
            .await
            .unwrap();
        let claimed = claim_next(&state).await.unwrap().unwrap();
        assert_eq!(claimed.id, "next");
    }

    #[tokio::test]
    async fn cancelled_runtime_keeps_occupancy_until_remote_effects_settle_and_uncertainty_is_acknowledged(
    ) {
        let state = state().await;
        sqlx::query("INSERT INTO groups(id,owner_id,name,status,created_at,updated_at) VALUES('group-1','owner-1','Team','active','now','now')").execute(state.db.pool()).await.unwrap();
        sqlx::query("INSERT INTO threads(id,group_id,status,created_at,updated_at) VALUES('thread-current','group-1','active','now','now')").execute(state.db.pool()).await.unwrap();
        sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('turn-current','thread-current','group-1','cancelled','bounded','{}','{}','now','now')").execute(state.db.pool()).await.unwrap();
        sqlx::query("INSERT INTO computers(id,owner_id,binding_kind,binding_id,name,created_at) VALUES('computer','owner-1','team','group-1','Computer','now')").execute(state.db.pool()).await.unwrap();
        job(
            &state,
            "current",
            "thread-current",
            NOW,
            None,
            "cancelled",
            None,
            1,
            1,
        )
        .await;
        sqlx::query("INSERT INTO computer_operations(id,turn_id,computer_id,status) VALUES('operation','turn-current','computer','running')").execute(state.db.pool()).await.unwrap();
        let worker_state = state.clone();
        let job=sqlx::query_as::<_,super::Job>("SELECT id,owner_id,group_id,thread_id,turn_id,stream_id,command_name,payload_json,generation FROM execution_jobs WHERE id='current'").fetch_one(state.db.pool()).await.unwrap();
        let mut worker = tokio::spawn(async move {
            super::finish_job(&worker_state, &job, crate::runtime::TurnOutcome::Cancelled).await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), &mut worker)
                .await
                .is_err()
        );
        let occupied: (i64, i64) = sqlx::query_as(
            "SELECT occupies_group,worker_active FROM execution_jobs WHERE id='current'",
        )
        .fetch_one(state.db.pool())
        .await
        .unwrap();
        assert_eq!(occupied, (1, 1));
        sqlx::query("UPDATE computer_operations SET status='uncertain' WHERE id='operation'")
            .execute(state.db.pool())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let job: (String, i64, i64) = sqlx::query_as(
            "SELECT status,occupies_group,worker_active FROM execution_jobs WHERE id='current'",
        )
        .fetch_one(state.db.pool())
        .await
        .unwrap();
        assert_eq!(job, ("interrupted".into(), 1, 0));
        super::accept(
            &state,
            "owner-1",
            super::Command {
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "turn.acknowledge_stopped".into(),
                target: serde_json::json!({"turn_id":"turn-current"}),
                payload: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM computer_operations WHERE id='operation'")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(status, "acknowledged");
        let occupied: i64 =
            sqlx::query_scalar("SELECT occupies_group FROM execution_jobs WHERE id='current'")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(occupied, 0);
    }
}
