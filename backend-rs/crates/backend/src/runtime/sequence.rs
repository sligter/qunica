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
use sqlx::SqlitePool;
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
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;

        let next_seq: i64 = sqlx::query_scalar("SELECT next_seq FROM threads WHERE id = ?")
            .bind(thread_id)
            .fetch_one(&mut *tx)
            .await?;

        sqlx::query(
            "INSERT INTO messages \
             (id, thread_id, group_id, seq, sender_type, sender_id, message_type, content, \
              status, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'visible', ?)",
        )
        .bind(&message.id)
        .bind(thread_id)
        .bind(group_id)
        .bind(next_seq)
        .bind(&message.sender_type)
        .bind(&message.sender_id)
        .bind(&message.message_type)
        .bind(&message.content)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query("UPDATE threads SET next_seq = ?, updated_at = ? WHERE id = ?")
            .bind(next_seq + 1)
            .bind(&now)
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;

        insert_stream_event(&mut tx, thread_id, event, &now).await?;

        tx.commit().await?;
        Ok(next_seq)
    }

    /// Persist a durable stream event with no associated message row (terminal
    /// markers such as `agent_silent`, `waiting_for_user` and `silence`).
    pub async fn persist_event(
        &self,
        thread_id: &str,
        event: &StreamEvent<Value>,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        insert_stream_event(&mut tx, thread_id, event, &now).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn insert_stream_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
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
