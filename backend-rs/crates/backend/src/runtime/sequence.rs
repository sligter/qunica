//! Per-thread monotonic sequence allocation and durable persistence.
//!
//! Chat ordering never relies on wall-clock timestamps: every persisted message
//! draws a strictly increasing sequence from `threads.next_seq`, allocated and
//! advanced inside a single transaction. Concurrent streams on the same SQLite
//! database are serialized behind one async write lock so a sequence read can
//! never interleave with another writer's update.

use std::sync::Arc;

use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use uuid::Uuid;

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
    ) -> anyhow::Result<i64> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;

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
        Ok(next_seq)
    }

    /// Append resume output to an existing interrupted message and keep the
    /// thread in the supplied state.
    pub async fn append_interrupted_message(
        &self,
        thread_id: &str,
        message_id: &str,
        addition: &str,
        thread_status: &str,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(
            "UPDATE messages \
             SET content = COALESCE(content, '') || ? \
             WHERE id = ? AND thread_id = ? AND status = 'interrupted'",
        )
        .bind(addition)
        .bind(message_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!("interrupted message not found"));
        }

        sqlx::query("UPDATE threads SET status = ?, updated_at = ? WHERE id = ?")
            .bind(thread_status)
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
        message_event: &StreamEvent<Value>,
        done_event: &StreamEvent<Value>,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(
            "UPDATE messages \
             SET content = ?, status = 'visible' \
             WHERE id = ? AND thread_id = ? AND status = 'interrupted'",
        )
        .bind(content)
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
        sqlx::query("UPDATE threads SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now)
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Persist a durable stream event with no associated message row (terminal
    /// markers such as `agent_silent`, `waiting_for_user` and `silence`).
    pub async fn persist_event(
        &self,
        thread_id: &str,
        event: &StreamEvent<Value>,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        persist_event_in_tx(&mut tx, thread_id, event).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Persist an ordered event group atomically. Scheduler terminal markers
    /// use this so replay can never observe a terminal event without its
    /// transport-level `done` event.
    pub async fn persist_events(
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

pub(crate) async fn persist_event_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    event: &StreamEvent<Value>,
) -> anyhow::Result<()> {
    let now = now_rfc3339();
    insert_stream_event(tx, thread_id, event, &now).await
}

async fn insert_stream_event(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    event: &StreamEvent<Value>,
    now: &str,
) -> anyhow::Result<()> {
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

/// Render a [`StreamEventKind`] as its snake_case wire string (without the JSON
/// quotes `serde_json::to_string` would add).
fn kind_str(kind: &StreamEventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
