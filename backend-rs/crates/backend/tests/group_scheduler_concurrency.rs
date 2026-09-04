use std::sync::Arc;

use qunica_backend::{
    db::Db,
    runtime::{
        group_scheduler::{
            ActionKind, DispatchOutput, DispatchStatus, FinishDispatch, NewDispatch, NewTurn,
            SchedulerStore, SchedulerStoreError, SelectionReason, TurnStatus,
        },
        sequence::{NewMessage, SequenceAllocator},
        StreamEvent, StreamEventKind,
    },
};
use serde_json::json;
use tokio::sync::{Barrier, Mutex};
use uuid::Uuid;

const NOW: &str = "2026-07-14T00:00:00Z";

#[tokio::test]
async fn cancellation_prevents_an_old_dispatch_from_appending_output() {
    let fixture = Fixture::new().await;
    fixture.create_running_turn("turn-old").await;
    fixture.queue_and_start("dispatch-old", "turn-old").await;

    fixture.store.cancel_turn("turn-old").await.unwrap();
    let error = fixture
        .store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-old".to_owned(),
            next: DispatchStatus::Completed,
            artifact: None,
            total_tokens: 3,
            failure_code: None,
            output: Some(fixture.output("dispatch-old-message")),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SchedulerStoreError::TransitionConflict {
            entity: "dispatch",
            actual: Some(ref status),
            ..
        } if status == "interrupted"
    ));

    let trace = fixture.store.load_turn_trace("turn-old").await.unwrap();
    assert_eq!(trace.turn.status, TurnStatus::Cancelled);
    assert_eq!(trace.dispatches[0].status, DispatchStatus::Interrupted);
    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(message_count, 0);
}

#[tokio::test]
async fn superseding_the_active_turn_allows_a_replacement_for_the_same_thread() {
    let fixture = Fixture::new().await;
    fixture.create_running_turn("turn-old").await;
    fixture.queue_and_start("dispatch-old", "turn-old").await;

    let superseded = fixture
        .store
        .supersede_active_turn_for_thread(&fixture.thread_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(superseded.status, TurnStatus::Superseded);
    let old = fixture.store.load_turn_trace("turn-old").await.unwrap();
    assert_eq!(old.dispatches[0].status, DispatchStatus::Interrupted);

    fixture
        .store
        .create_turn(fixture.turn("turn-replacement"))
        .await
        .unwrap();
    let replacement = fixture
        .store
        .load_turn_trace("turn-replacement")
        .await
        .unwrap();
    assert_eq!(replacement.turn.status, TurnStatus::Pending);
}

#[tokio::test]
async fn concurrent_turn_creation_keeps_one_active_turn_per_thread() {
    let fixture = Fixture::new().await;
    let barrier = Arc::new(Barrier::new(2));

    let first_store = fixture.store.clone();
    let first_barrier = barrier.clone();
    let first_turn = fixture.turn("turn-concurrent-a");
    let first = tokio::spawn(async move {
        first_barrier.wait().await;
        first_store.create_turn(first_turn).await
    });

    let second_store = fixture.store.clone();
    let second_barrier = barrier;
    let second_turn = fixture.turn("turn-concurrent-b");
    let second = tokio::spawn(async move {
        second_barrier.wait().await;
        second_store.create_turn(second_turn).await
    });

    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(SchedulerStoreError::ActiveTurnExists { .. })))
            .count(),
        1
    );

    let active_turns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_turns \
         WHERE thread_id = ? AND status IN ('pending', 'running', 'waiting_for_user')",
    )
    .bind(&fixture.thread_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(active_turns, 1);
}

#[tokio::test]
async fn finishing_a_dispatch_retries_a_transient_sqlite_write_lock() {
    let (fixture, _directory) = Fixture::new_file().await;
    fixture.create_running_turn("turn-locked").await;
    fixture
        .queue_and_start("dispatch-locked", "turn-locked")
        .await;

    // busy_timeout is connection-local. Configure every pooled connection so
    // the first attempt fails promptly instead of waiting out the blocker.
    let mut connections = Vec::new();
    for _ in 0..8 {
        connections.push(fixture.pool.acquire().await.unwrap());
    }
    for connection in &mut connections {
        sqlx::query("PRAGMA busy_timeout = 1")
            .execute(&mut **connection)
            .await
            .unwrap();
    }
    drop(connections);

    let mut blocker = fixture.pool.begin().await.unwrap();
    sqlx::query("UPDATE groups SET updated_at = 'blocked' WHERE id = ?")
        .bind(&fixture.group_id)
        .execute(&mut *blocker)
        .await
        .unwrap();

    let store = fixture.store.clone();
    let output = fixture.output("message-after-lock");
    let ready = Arc::new(Barrier::new(2));
    let finish_ready = ready.clone();
    let finish = tokio::spawn(async move {
        finish_ready.wait().await;
        store
            .finish_dispatch(FinishDispatch {
                dispatch_id: "dispatch-locked".to_owned(),
                next: DispatchStatus::Completed,
                artifact: None,
                total_tokens: 7,
                failure_code: None,
                output: Some(output),
            })
            .await
    });
    ready.wait().await;
    let started = std::time::Instant::now();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    blocker.rollback().await.unwrap();

    let finished = finish.await.unwrap().unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(20));
    assert_eq!(finished.status, DispatchStatus::Completed);
    assert_eq!(
        finished.output_message_id.as_deref(),
        Some("message-after-lock")
    );
}

#[tokio::test]
async fn persisting_a_stream_event_retries_a_transient_sqlite_write_lock() {
    let (fixture, _directory) = Fixture::new_file().await;

    // busy_timeout is connection-local. Configure every pooled connection so
    // the first attempt fails promptly instead of waiting out the blocker.
    let mut connections = Vec::new();
    for _ in 0..8 {
        connections.push(fixture.pool.acquire().await.unwrap());
    }
    for connection in &mut connections {
        sqlx::query("PRAGMA busy_timeout = 1")
            .execute(&mut **connection)
            .await
            .unwrap();
    }
    drop(connections);

    let mut blocker = fixture.pool.begin().await.unwrap();
    sqlx::query("UPDATE groups SET updated_at = 'blocked' WHERE id = ?")
        .bind(&fixture.group_id)
        .execute(&mut *blocker)
        .await
        .unwrap();

    let allocator = SequenceAllocator::new(fixture.pool.clone(), Arc::new(Mutex::new(())));
    let event = StreamEvent::new(
        Uuid::new_v4(),
        0,
        StreamEventKind::AcpAgentRun,
        json!({"status": "completed"}),
    );
    let ready = Arc::new(Barrier::new(2));
    let persist_ready = ready.clone();
    let thread_id = fixture.thread_id.clone();
    let persist = tokio::spawn(async move {
        persist_ready.wait().await;
        allocator.persist_event(&thread_id, &event).await
    });
    ready.wait().await;
    let started = std::time::Instant::now();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    blocker.rollback().await.unwrap();

    persist.await.unwrap().unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(20));
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(event_count, 1);
}

/// `cancel_turn` reads the turn before it updates it, and it has no retry loop.
///
/// Under `BEGIN DEFERRED` that upgrade is refused the instant another
/// connection holds SQLite's writer — the error comes back as "database is
/// locked" without `busy_timeout` ever being consulted, and the cancel fails
/// outright. Opening the transaction with `BEGIN IMMEDIATE` makes the same call
/// queue behind the other writer instead.
#[tokio::test]
async fn cancelling_a_turn_waits_out_a_concurrent_writer() {
    let (fixture, _directory) = Fixture::new_file().await;
    fixture.create_running_turn("turn-contended").await;

    // Stands in for an API handler's transaction: it holds the writer and does
    // not take the runtime write lock, so nothing serializes the two in-process.
    let mut holder = qunica_backend::db::begin_write(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE groups SET updated_at = 'held' WHERE id = ?")
        .bind(&fixture.group_id)
        .execute(&mut *holder)
        .await
        .unwrap();

    let (cancelled, ()) = tokio::join!(fixture.store.cancel_turn("turn-contended"), async {
        // Long enough that the cancel has certainly reached the database and is
        // blocked on the writer rather than racing past it.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        holder.commit().await.unwrap();
    });

    assert_eq!(cancelled.unwrap().status, TurnStatus::Cancelled);
}

struct Fixture {
    pool: sqlx::SqlitePool,
    store: SchedulerStore,
    group_id: String,
    thread_id: String,
    agent_id: String,
}

impl Fixture {
    async fn new() -> Self {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        Self::from_pool(db.pool().clone()).await
    }

    async fn new_file() -> (Self, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let mut path = directory
            .path()
            .join("scheduler-lock.sqlite3")
            .to_string_lossy()
            .replace('\\', "/");
        if !path.starts_with('/') && !path.starts_with("//") {
            path = format!("/{path}");
        }
        let db = Db::connect(&format!("sqlite://{path}?mode=rwc"))
            .await
            .unwrap();
        db.migrate().await.unwrap();
        (Self::from_pool(db.pool().clone()).await, directory)
    }

    async fn from_pool(pool: sqlx::SqlitePool) -> Self {
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, name, created_at, updated_at) \
             VALUES ('user-1', 'owner@example.test', 'hash', 'Owner', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO groups (id, owner_id, name, created_at, updated_at) \
             VALUES ('group-1', 'user-1', 'Team', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO threads (id, group_id, created_at, updated_at) \
             VALUES ('thread-1', 'group-1', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agents (id, owner_id, name, system_prompt, created_at, updated_at) \
             VALUES ('agent-1', 'user-1', 'Agent', 'Help', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        Self {
            pool: pool.clone(),
            store: SchedulerStore::new(pool, Arc::new(Mutex::new(()))),
            group_id: "group-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            agent_id: "agent-1".to_owned(),
        }
    }

    fn turn(&self, id: &str) -> NewTurn {
        NewTurn {
            id: id.to_owned(),
            thread_id: self.thread_id.clone(),
            group_id: self.group_id.clone(),
            trigger_message_id: None,
            scheduler_strategy: "deterministic".to_owned(),
            config_snapshot: json!({"max_agent_steps": 8}),
            topology_snapshot: json!({"mode": "mesh"}),
        }
    }

    async fn create_running_turn(&self, turn_id: &str) {
        self.store.create_turn(self.turn(turn_id)).await.unwrap();
        self.store
            .transition_turn(turn_id, TurnStatus::Pending, TurnStatus::Running, None)
            .await
            .unwrap();
    }

    async fn queue_and_start(&self, dispatch_id: &str, turn_id: &str) {
        self.store
            .queue_dispatch(NewDispatch {
                id: dispatch_id.to_owned(),
                turn_id: turn_id.to_owned(),
                parent_dispatch_id: None,
                source_agent_id: None,
                target_agent_id: self.agent_id.clone(),
                selection_reason: SelectionReason::DeterministicOrder,
                action_kind: ActionKind::Speak,
                hop: 0,
                input_message_id: None,
            })
            .await
            .unwrap();
        self.store.start_dispatch(dispatch_id).await.unwrap();
    }

    fn output(&self, message_id: &str) -> DispatchOutput {
        let message = NewMessage {
            id: message_id.to_owned(),
            sender_type: "agent".to_owned(),
            sender_id: Some(self.agent_id.clone()),
            message_type: "text".to_owned(),
            content: "late output".to_owned(),
            content_json: None,
        };
        DispatchOutput {
            thread_id: self.thread_id.clone(),
            group_id: self.group_id.clone(),
            event: StreamEvent::new(
                Uuid::new_v4(),
                0,
                StreamEventKind::AgentMessage,
                json!({"message_id": message.id, "content": message.content}),
            ),
            message,
        }
    }
}
