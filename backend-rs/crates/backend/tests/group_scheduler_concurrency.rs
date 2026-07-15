use std::sync::Arc;

use ag_swarmer_backend::{
    db::Db,
    runtime::{
        group_scheduler::{
            ActionKind, DispatchOutput, DispatchStatus, FinishDispatch, NewDispatch, NewTurn,
            SchedulerStore, SchedulerStoreError, SelectionReason, TurnStatus,
        },
        sequence::NewMessage,
        StreamEvent, StreamEventKind,
    },
};
use serde_json::json;
use tokio::sync::Mutex;
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
        let pool = db.pool().clone();
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
