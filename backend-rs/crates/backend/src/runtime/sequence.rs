//! Per-thread monotonic sequence allocation and durable persistence.
//!
//! Chat ordering never relies on wall-clock timestamps: every persisted message
//! draws a strictly increasing sequence from `threads.next_seq`, allocated and
//! advanced inside a single transaction. Concurrent streams on the same SQLite
//! database are serialized behind one async write lock so a sequence read can
//! never interleave with another writer's update.

use std::sync::Arc;
use std::time::Duration;

use qunica_domain::events::{StreamEvent, StreamEventKind};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use uuid::Uuid;

const SQLITE_WRITE_ATTEMPTS: usize = 3;
const SQLITE_WRITE_RETRY_DELAY: Duration = Duration::from_millis(25);

/// A message row to persist at a freshly allocated thread sequence.
pub struct NewMessage {
    pub id: String,
    pub sender_type: String,
    pub sender_id: Option<String>,
    pub message_type: String,
    pub content: String,
    /// Serialized structured turn data (reasoning segments, tool calls, context
    /// usage). `None` for rows that carry no structured data (e.g. plain user
    /// messages); the DB column is then `NULL` and readers fall back to the
    /// plain `content` text.
    pub content_json: Option<String>,
}

/// Coordinates writes to the chat/runtime tables.
///
/// All mutations acquire `write_lock` and run in one transaction, so the
/// `threads.next_seq` read-modify-write cycle is atomic with the inserts it
/// guards.
#[derive(Clone)]
pub struct SequenceAllocator {
    pool: SqlitePool,
    write_lock: Arc<Mutex<()>>,
}

impl SequenceAllocator {
    pub fn new(pool: SqlitePool, write_lock: Arc<Mutex<()>>) -> Self {
        Self { pool, write_lock }
    }

    /// Persist a durable message and the stream event that announced it in one
    /// transaction.
    ///
    /// The message is stored at the thread's next free sequence (then advanced);
    /// the stream event keeps its own stream-local sequence. Returns the thread
    /// sequence assigned to the message.
    pub async fn persist_message_with_event(
        &self,
        thread_id: &str,
        group_id: &str,
        message: &NewMessage,
        event: &StreamEvent<Value>,
    ) -> anyhow::Result<i64> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let next_seq =
            persist_message_with_event_in_tx(&mut tx, thread_id, group_id, message, event).await?;
        tx.commit().await?;
        Ok(next_seq)
    }

    /// Persist a partial agent message at the thread's next sequence and pause
    /// the thread. No stream event is written because the client has already
    /// disconnected before the final `agent_message` checkpoint.
    pub async fn persist_interrupted_message(
        &self,
        thread_id: &str,
        group_id: &str,
        message: &NewMessage,
        active_dispatch_id: Option<&str>,
    ) -> anyhow::Result<Option<i64>> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        ensure_thread_writable(&mut tx, thread_id).await?;

        if let Some(dispatch_id) = active_dispatch_id {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS(\
                    SELECT 1 FROM agent_dispatches d \
                    JOIN group_turns t ON t.id = d.turn_id \
                    WHERE d.id = ? AND d.status = 'running' AND t.status = 'running' \
                      AND t.thread_id = ? AND t.group_id = ?\
                 )",
            )
            .bind(dispatch_id)
            .bind(thread_id)
            .bind(group_id)
            .fetch_one(&mut *tx)
            .await?;
            if !active {
                return Ok(None);
            }
        }

        let next_seq: i64 = sqlx::query_scalar("SELECT next_seq FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(&mut *tx)
            .await?;

        sqlx::query(
            "INSERT INTO messages \
             (id, thread_id, group_id, seq, sender_type, sender_id, message_type, content, \
              content_json, status, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'interrupted', ?)",
        )
        .bind(&message.id)
        .bind(thread_id)
        .bind(group_id)
        .bind(next_seq)
        .bind(&message.sender_type)
        .bind(&message.sender_id)
        .bind(&message.message_type)
        .bind(&message.content)
        .bind(&message.content_json)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE threads SET next_seq = ?, status = 'paused', updated_at = ? WHERE id = ?",
        )
        .bind(next_seq + 1)
        .bind(&now)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(next_seq))
    }

    /// Replace an interrupted checkpoint in place and pause its thread.
    pub async fn checkpoint_interrupted_message(
        &self,
        thread_id: &str,
        message_id: &str,
        content: &str,
        content_json: Option<&str>,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        ensure_thread_writable(&mut tx, thread_id).await?;

        let result = sqlx::query(
            "UPDATE messages \
             SET content = ?, content_json = ? \
             WHERE id = ? AND thread_id = ? AND status = 'interrupted'",
        )
        .bind(content)
        .bind(content_json)
        .bind(message_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!("interrupted message not found"));
        }

        sqlx::query("UPDATE threads SET status = 'paused', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Complete an interrupted message in place and persist the durable
    /// `agent_message` and `done` events for the resumed stream.
    pub async fn complete_interrupted_message_with_events(
        &self,
        thread_id: &str,
        message_id: &str,
        content: &str,
        content_json: Option<&str>,
        message_event: &StreamEvent<Value>,
        done_event: &StreamEvent<Value>,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        ensure_thread_writable(&mut tx, thread_id).await?;

        let result = sqlx::query(
            "UPDATE messages \
             SET content = ?, content_json = ?, status = 'visible' \
             WHERE id = ? AND thread_id = ? AND status = 'interrupted'",
        )
        .bind(content)
        .bind(content_json)
        .bind(message_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!("interrupted message not found"));
        }

        sqlx::query("UPDATE threads SET status = 'active', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;

        insert_stream_event(&mut tx, thread_id, message_event, &now).await?;
        insert_stream_event(&mut tx, thread_id, done_event, &now).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Update only the thread status under the same write lock used for other
    /// runtime mutations.
    pub async fn set_thread_status(&self, thread_id: &str, status: &str) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let result = sqlx::query(
            "UPDATE threads SET status = ?, updated_at = ? \
             WHERE id = ? AND status NOT IN ('cleared', 'archived')",
        )
        .bind(status)
        .bind(&now)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!("thread is not writable"));
        }
        Ok(())
    }

    /// Supersede a paused thread so a new user message can start a fresh turn.
    ///
    /// A thread is paused because its last turn was interrupted (provider
    /// failure, disconnect, or cancel after partial output). Sending a new
    /// message should not dead-end in a 409: the interrupted checkpoint stays
    /// in history as a normal visible message and the thread returns to
    /// `active`, so the new turn starts cleanly. Returns `false` when the
    /// thread was not paused.
    pub async fn supersede_paused_thread(&self, thread_id: &str) -> anyhow::Result<bool> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            "UPDATE threads SET status = 'active', updated_at = ? \
             WHERE id = ? AND status = 'paused'",
        )
        .bind(&now)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 1 {
            sqlx::query(
                "UPDATE messages SET status = 'visible' \
                 WHERE thread_id = ? AND status = 'interrupted'",
            )
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    /// Atomically claim a paused thread for a detached resume task.
    pub async fn claim_paused_thread(&self, thread_id: &str) -> anyhow::Result<bool> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let result = sqlx::query(
            "UPDATE threads \
             SET status = 'running', updated_at = ? \
             WHERE id = ? AND status = 'paused'",
        )
        .bind(&now)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Persist a durable stream event with no associated message row (terminal
    /// markers such as `agent_silent`, `waiting_for_user` and `silence`).
    pub async fn persist_event(
        &self,
        thread_id: &str,
        event: &StreamEvent<Value>,
    ) -> anyhow::Result<()> {
        self.persist_events(thread_id, std::slice::from_ref(event))
            .await
    }

    /// Persist an ordered event group atomically. Scheduler terminal markers
    /// use this so replay can never observe a terminal event without its
    /// transport-level `done` event.
    pub async fn persist_events(
        &self,
        thread_id: &str,
        events: &[StreamEvent<Value>],
    ) -> anyhow::Result<()> {
        for attempt in 1..=SQLITE_WRITE_ATTEMPTS {
            match self.persist_events_once(thread_id, events).await {
                Err(error)
                    if attempt < SQLITE_WRITE_ATTEMPTS && is_transient_sqlite_lock(&error) =>
                {
                    tracing::warn!(
                        thread_id,
                        event_count = events.len(),
                        attempt,
                        error = %error,
                        "retrying stream event persistence after SQLite lock"
                    );
                    tokio::time::sleep(SQLITE_WRITE_RETRY_DELAY).await;
                }
                Err(error) => {
                    tracing::error!(
                        thread_id,
                        event_count = events.len(),
                        error = %error,
                        "failed to persist stream event"
                    );
                    return Err(error);
                }
                Ok(()) => return Ok(()),
            }
        }
        unreachable!("stream event retry loop always returns")
    }

    async fn persist_events_once(
        &self,
        thread_id: &str,
        events: &[StreamEvent<Value>],
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        for event in events {
            insert_stream_event(&mut tx, thread_id, event, &now).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

pub(crate) async fn persist_message_with_event_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    group_id: &str,
    message: &NewMessage,
    event: &StreamEvent<Value>,
) -> anyhow::Result<i64> {
    let now = now_rfc3339();
    ensure_thread_writable(tx, thread_id).await?;
    let next_seq: i64 = sqlx::query_scalar("SELECT next_seq FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_one(&mut **tx)
        .await?;

    sqlx::query(
        "INSERT INTO messages \
         (id, thread_id, group_id, seq, sender_type, sender_id, message_type, content, \
          content_json, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'visible', ?)",
    )
    .bind(&message.id)
    .bind(thread_id)
    .bind(group_id)
    .bind(next_seq)
    .bind(&message.sender_type)
    .bind(&message.sender_id)
    .bind(&message.message_type)
    .bind(&message.content)
    .bind(&message.content_json)
    .bind(&now)
    .execute(&mut **tx)
    .await?;

    sqlx::query("UPDATE threads SET next_seq = ?, updated_at = ? WHERE id = ?")
        .bind(next_seq + 1)
        .bind(&now)
        .bind(thread_id)
        .execute(&mut **tx)
        .await?;

    insert_stream_event(tx, thread_id, event, &now).await?;
    Ok(next_seq)
}

async fn insert_stream_event(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    event: &StreamEvent<Value>,
    now: &str,
) -> anyhow::Result<()> {
    ensure_thread_writable(tx, thread_id).await?;
    sqlx::query(
        "INSERT INTO stream_events \
         (id, stream_id, thread_id, seq, event_id, kind, payload_json, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(event.stream_id.to_string())
    .bind(thread_id)
    .bind(event.seq)
    .bind(&event.event_id)
    .bind(kind_str(&event.kind))
    .bind(serde_json::to_string(&event.payload)?)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_thread_writable(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> anyhow::Result<()> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(&mut **tx)
        .await?;
    if matches!(status.as_deref(), None | Some("cleared" | "archived")) {
        return Err(anyhow::anyhow!("thread is not writable"));
    }
    Ok(())
}

/// Render a [`StreamEventKind`] as its snake_case wire string (without the JSON
/// quotes `serde_json::to_string` would add).
fn kind_str(kind: &StreamEventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn is_transient_sqlite_lock(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sqlx::Error>()
        .and_then(sqlx::Error::as_database_error)
        .and_then(|error| error.code())
        .and_then(|code| code.parse::<i32>().ok())
        .is_some_and(|code| matches!(code & 0xff, 5 | 6))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
