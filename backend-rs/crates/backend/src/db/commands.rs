//! Shared durable command writes for HTTP/WS, automation and Router callers.
//!
//! Note: inputs are already authorized and prepared by the caller. The caller
//! owns the write lock, transaction commit/rollback and worker wake-up; starting
//! another transaction here would break atomic Router reports and occurrences.
//! See `specs/command-acceptance-boundary.md` for the compatibility contract.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::runtime::{group::MessageAttachment, group_scheduler::store::SchedulerStoreError};

#[derive(Debug, Error)]
pub(crate) enum CommandStoreError {
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("failed to cancel turn")]
    CancelTurn(#[source] SchedulerStoreError),
}

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

pub(crate) fn payload_hash(command: &Command) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            crate::canonical_json::to_vec(
                &json!({"name":command.name,"target":command.target,"payload":command.payload})
            )
            .unwrap()
        )
    )
}

/// A new message on a paused thread supersedes the interrupted checkpoint left
/// by a failed or disconnected turn, matching the legacy send path. Callers
/// answering a waiting job take their own branch, so this never silently
/// answers an approval gate.
pub(crate) async fn reopen_paused_thread_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread: &str,
    now: &str,
) -> Result<(), CommandStoreError> {
    let superseded = sqlx::query(
        "UPDATE threads SET status='active',updated_at=? WHERE id=? AND status='paused'",
    )
    .bind(now)
    .bind(thread)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if superseded == 1 {
        sqlx::query(
            "UPDATE messages SET status='visible' WHERE thread_id=? AND status='interrupted'",
        )
        .bind(thread)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
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
    /// `None` records the owner as the sender, as every human message does.
    pub author: Option<MessageAuthor<'a>>,
}

/// Who a server-written message is from. Router dispatches are written as the
/// router agent and task reports as `system`, so neither reads as user input.
pub(crate) struct MessageAuthor<'a> {
    pub sender_type: &'a str,
    pub sender_id: Option<&'a str>,
    pub message_type: &'a str,
    pub content_json: Option<String>,
}

/// Shared transaction core: timer/webhook acceptance also writes the message,
/// reserved turn, job and receipt before acknowledging any occurrence.
pub(crate) async fn insert_message_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: MessageAcceptance<'_>,
) -> Result<Value, CommandStoreError> {
    let MessageAcceptance {
        owner,
        command,
        digest,
        group,
        thread,
        prepared,
        now,
        reuse_job,
        author,
    } = input;
    let (sender_type, sender_id, message_type, content_json) = match author {
        Some(author) => (
            author.sender_type,
            author.sender_id,
            author.message_type,
            author.content_json,
        ),
        None => (
            "user",
            Some(owner),
            "text",
            crate::runtime::group::user_attachment_content_json(&prepared.attachments),
        ),
    };
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM group_turns WHERE thread_id=? AND status IN ('pending','running','waiting_for_user'))")
        .bind(thread).fetch_one(&mut **tx).await?;
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
        .bind(&command.request_id).bind(&command.request_id).fetch_one(&mut **tx).await?;
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
    .await?;
    // Note: accepting input makes it visible to clients, but only the claimant
    // may admit it into model history after the preceding worker has stopped.
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,sender_id,message_type,content,content_json,status,created_at,context_admitted) VALUES(?,?,?,?,?,?,?,?,?,'visible',?,0)")
        .bind(&message).bind(thread).bind(group).bind(seq).bind(sender_type).bind(sender_id).bind(message_type).bind(&prepared.content).bind(&content_json).bind(now).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,trigger_message_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,?,?,'bounded','{}','{}',?,?)")
        .bind(&turn).bind(thread).bind(group).bind(&message).bind(turn_status).bind(now).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE messages SET turn_id=? WHERE id=?")
        .bind(&turn)
        .bind(&message)
        .execute(&mut **tx)
        .await?;
    let payload = json!({"message_id":message,"thread_id":thread,"content":prepared.content,"attachments":prepared.attachments,"sender_type":sender_type,"sender_id":sender_id,"message_type":message_type});
    sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,0,?,'user_message',?,?)")
        .bind(Uuid::new_v4().to_string()).bind(&stream).bind(thread).bind(format!("{stream}:0")).bind(payload.to_string()).bind(now).execute(&mut **tx).await?;
    let response = json!({"request_id":command.request_id,"status":"accepted","job_id":job,"turn_id":turn,"stream_id":stream,"thread_id":thread});
    if reuse_job.is_some() {
        sqlx::query("UPDATE execution_jobs SET command_name='message.send',thread_id=?,turn_id=?,stream_id=?,message_id=?,payload_json=?,status='queued',error=NULL,updated_at=? WHERE id=?")
            .bind(thread).bind(&turn).bind(&stream).bind(&message).bind(serde_json::to_string(prepared).unwrap()).bind(now).bind(&job).execute(&mut **tx).await?;
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

pub(crate) async fn insert_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    command: &Command,
    digest: &str,
    response: &Value,
    now: &str,
) -> Result<(), CommandStoreError> {
    sqlx::query("INSERT INTO command_receipts(owner_id,request_id,command_name,payload_hash,response_json,created_at) VALUES(?,?,?,?,?,?)")
        .bind(owner).bind(&command.request_id).bind(&command.name).bind(digest).bind(response.to_string()).bind(now).execute(&mut **tx).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_job(
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
) -> Result<(), CommandStoreError> {
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,message_id,payload_json,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,'queued',?,?)")
        .bind(response["job_id"].as_str()).bind(owner).bind(&command.request_id).bind(&command.name).bind(group).bind(thread).bind(turn).bind(stream).bind(message).bind(payload).bind(now).bind(now)
        .execute(&mut **tx).await?;
    Ok(())
}

/// Write the same terminal pair a cancelled run leaves behind. `done` is
/// guarded, so replaying a recovery path cannot append a second terminal.
pub(crate) async fn append_stream_terminal(
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
    sqlx::query("UPDATE execution_jobs SET status='cancelled',occupies_thread=0,worker_active=0,updated_at=? WHERE id=? AND status='queued'")
        .bind(&now)
        .bind(job_id)
        .execute(&mut **tx)
        .await?;
    append_stream_terminal(tx, &stream, &thread, &turn, "superseded", &now).await?;
    Ok(())
}

/// Durable half of a turn cancellation; the caller signals the live worker
/// through the active-turn registry after committing.
pub(crate) async fn cancel_turn_effects_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    turn: &str,
    thread: &str,
) -> Result<(), CommandStoreError> {
    let queued: Vec<(String, String)> = sqlx::query_as(
        "SELECT stream_id,thread_id FROM execution_jobs WHERE turn_id=? AND status='queued'",
    )
    .bind(turn)
    .fetch_all(&mut **tx)
    .await?;
    crate::runtime::group_scheduler::store::cancel_turn_in_tx(tx, turn, &now())
        .await
        .map_err(CommandStoreError::CancelTurn)?;
    sqlx::query("UPDATE execution_jobs SET status='cancelled',occupies_thread=CASE WHEN worker_active=0 THEN 0 ELSE occupies_thread END,updated_at=? WHERE turn_id=? AND status IN ('queued','running','waiting_for_user')").bind(now()).bind(turn).execute(&mut **tx).await?;
    // Note: a thread can already contain a newer approval when an old turn's
    // cancel is delivered. The checkpoint identifies the question's own turn.
    sqlx::query("UPDATE pending_approvals SET status='cancelled',version=version+1,updated_at=? WHERE thread_id=? AND status='pending' AND EXISTS(SELECT 1 FROM messages m,json_each(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '{}' END,'$.tool_calls') c WHERE m.thread_id=pending_approvals.thread_id AND m.turn_id=? AND json_extract(c.value,'$.tool_call_id')=pending_approvals.tool_call_id)")
        .bind(now()).bind(thread).bind(turn).execute(&mut **tx).await?;
    for (stream, thread) in queued {
        append_stream_terminal(tx, &stream, &thread, turn, "user_cancelled", &now()).await?;
    }
    Ok(())
}

pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
#[cfg(test)]
#[path = "commands/tests.rs"]
mod tests;
