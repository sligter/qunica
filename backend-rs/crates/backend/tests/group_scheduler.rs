use std::sync::Arc;

use qunica_backend::{
    db::Db,
    runtime::{
        group_scheduler::{
            next_decision, ActionKind, BudgetLimits, DispatchOutput, DispatchStatus,
            FinishDispatch, NewDispatch, NewTurn, SchedulerDecision, SchedulerModelError,
            SchedulerStore, SchedulerStoreError, SelectionReason, TurnBudget, TurnReason,
            TurnStatus,
        },
        sequence::NewMessage,
        StreamEvent, StreamEventKind,
    },
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use uuid::Uuid;

const NOW: &str = "2026-07-11T00:00:00Z";

#[test]
fn moderator_decision_requires_two_legal_candidates() {
    let budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
    let one_candidate = ["a".to_owned()];
    assert!(matches!(
        next_decision(&budget, None, &[], &one_candidate, true, false),
        SchedulerDecision::Dispatch(ref dispatch)
            if dispatch.target_agent_id == "a"
                && dispatch.selection_reason == SelectionReason::DeterministicOrder
    ));

    let candidates = ["a".to_owned(), "b".to_owned()];

    assert!(matches!(
        next_decision(&budget, None, &[], &candidates, true, false),
        SchedulerDecision::RequestModerator
    ));
}

struct Fixture {
    pool: SqlitePool,
    store: SchedulerStore,
    thread_id: String,
    group_id: String,
    agent_a: String,
    agent_b: String,
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
        for (id, name) in [("agent-a", "Agent A"), ("agent-b", "Agent B")] {
            sqlx::query(
                "INSERT INTO agents \
                 (id, owner_id, name, system_prompt, created_at, updated_at) \
                 VALUES (?, 'user-1', ?, 'Help', ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(NOW)
            .bind(NOW)
            .execute(&pool)
            .await
            .unwrap();
        }

        let store = SchedulerStore::new(pool.clone(), Arc::new(Mutex::new(())));
        Self {
            pool,
            store,
            thread_id: "thread-1".to_owned(),
            group_id: "group-1".to_owned(),
            agent_a: "agent-a".to_owned(),
            agent_b: "agent-b".to_owned(),
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

    fn dispatch(&self, id: &str, turn_id: &str, target_agent_id: &str) -> NewDispatch {
        NewDispatch {
            id: id.to_owned(),
            turn_id: turn_id.to_owned(),
            parent_dispatch_id: None,
            source_agent_id: None,
            target_agent_id: target_agent_id.to_owned(),
            selection_reason: SelectionReason::DeterministicOrder,
            action_kind: ActionKind::Speak,
            hop: 0,
            input_message_id: None,
        }
    }
}

#[tokio::test]
async fn store_enforces_one_active_turn_per_thread() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();

    let error = fixture
        .store
        .create_turn(fixture.turn("turn-2"))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SchedulerStoreError::ActiveTurnExists { ref thread_id }
            if thread_id == &fixture.thread_id
    ));

    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();
    let completed = fixture
        .store
        .transition_turn(
            "turn-1",
            TurnStatus::Running,
            TurnStatus::Completed,
            Some("silence"),
        )
        .await
        .unwrap();
    assert_eq!(completed.termination_reason, Some(TurnReason::Silence));

    let second = fixture
        .store
        .create_turn(fixture.turn("turn-2"))
        .await
        .unwrap();
    assert_eq!(second.id, "turn-2");
}

#[tokio::test]
async fn store_supersede_and_create_replaces_the_active_turn_atomically() {
    let fixture = Fixture::new().await;
    let (nothing_superseded, first) = fixture
        .store
        .supersede_and_create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    assert!(nothing_superseded.is_none());
    assert_eq!(first.status, TurnStatus::Pending);

    let (superseded, second) = fixture
        .store
        .supersede_and_create_turn(fixture.turn("turn-2"))
        .await
        .unwrap();
    let superseded = superseded.expect("the first turn is superseded, not rejected");
    assert_eq!(superseded.id, "turn-1");
    assert_eq!(superseded.status, TurnStatus::Superseded);
    assert_eq!(superseded.termination_reason, Some(TurnReason::Superseded));
    assert_eq!(second.id, "turn-2");
    assert_eq!(second.status, TurnStatus::Pending);
}

#[tokio::test]
async fn store_supersede_and_create_leaves_no_turn_behind_when_the_insert_fails() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .supersede_and_create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();

    let mut invalid = fixture.turn("turn-2");
    invalid.trigger_message_id = Some("missing-message".to_owned());
    assert!(fixture
        .store
        .supersede_and_create_turn(invalid)
        .await
        .is_err());

    // The rolled-back transaction must not have superseded the live turn.
    let status: String = sqlx::query_scalar("SELECT status FROM group_turns WHERE id = 'turn-1'")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(status, TurnStatus::Pending.as_str());
    let turn_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM group_turns")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(turn_count, 1);
}

#[tokio::test]
async fn store_rejects_unknown_turn_reason_without_changing_state() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();
    let before = fixture.store.load_turn_trace("turn-1").await.unwrap();

    let error = fixture
        .store
        .transition_turn(
            "turn-1",
            TurnStatus::Running,
            TurnStatus::Completed,
            Some("not_a_protocol_reason"),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SchedulerStoreError::Model(SchedulerModelError::UnknownTurnReason(ref value))
            if value == "not_a_protocol_reason"
    ));

    let after = fixture.store.load_turn_trace("turn-1").await.unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn store_finishes_dispatch_with_message_and_event_atomically() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();
    fixture
        .store
        .queue_dispatch(fixture.dispatch("dispatch-1", "turn-1", &fixture.agent_a))
        .await
        .unwrap();
    fixture.store.start_dispatch("dispatch-1").await.unwrap();

    let stream_id = Uuid::new_v4();
    let finished = fixture
        .store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-1".to_owned(),
            next: DispatchStatus::Completed,
            artifact: Some(json!({"summary": "done"})),
            total_tokens: 42,
            failure_code: None,
            output: Some(DispatchOutput {
                thread_id: fixture.thread_id.clone(),
                group_id: fixture.group_id.clone(),
                message: NewMessage {
                    id: "message-1".to_owned(),
                    sender_type: "agent".to_owned(),
                    sender_id: Some(fixture.agent_a.clone()),
                    message_type: "text".to_owned(),
                    content: "Finished".to_owned(),
                    content_json: None,
                },
                event: StreamEvent::new(
                    stream_id,
                    1,
                    StreamEventKind::AgentMessage,
                    json!({"message_id": "message-1"}),
                ),
            }),
        })
        .await
        .unwrap();

    assert_eq!(finished.status, DispatchStatus::Completed);
    assert_eq!(finished.output_message_id.as_deref(), Some("message-1"));
    let message_links: (Option<String>, Option<String>, i64) =
        sqlx::query_as("SELECT turn_id, dispatch_id, seq FROM messages WHERE id = 'message-1'")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(message_links.0.as_deref(), Some("turn-1"));
    assert_eq!(message_links.1.as_deref(), Some("dispatch-1"));
    assert_eq!(message_links.2, 1);
    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM stream_events WHERE event_id = ?")
            .bind(format!("{stream_id}:1"))
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(event_count, 1);
    let payload_json: String =
        sqlx::query_scalar("SELECT payload_json FROM stream_events WHERE event_id = ?")
            .bind(format!("{stream_id}:1"))
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload_json).unwrap()["message_id"],
        "message-1"
    );
}

#[tokio::test]
async fn store_cancels_only_queued_dispatches_and_orders_trace() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    for (id, target) in [
        ("dispatch-z", &fixture.agent_a),
        ("dispatch-a", &fixture.agent_b),
        ("dispatch-m", &fixture.agent_a),
    ] {
        fixture
            .store
            .queue_dispatch(fixture.dispatch(id, "turn-1", target))
            .await
            .unwrap();
    }
    fixture.store.start_dispatch("dispatch-a").await.unwrap();
    sqlx::query("UPDATE agent_dispatches SET created_at = ? WHERE turn_id = 'turn-1'")
        .bind(NOW)
        .execute(&fixture.pool)
        .await
        .unwrap();

    assert_eq!(
        fixture
            .store
            .cancel_queued_dispatches("turn-1")
            .await
            .unwrap(),
        2
    );
    let trace = fixture.store.load_turn_trace("turn-1").await.unwrap();
    assert_eq!(
        trace
            .dispatches
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["dispatch-z", "dispatch-a", "dispatch-m"]
    );
    assert_eq!(trace.dispatches[0].status, DispatchStatus::Cancelled);
    assert_eq!(trace.dispatches[1].status, DispatchStatus::Running);
    assert_eq!(trace.dispatches[2].status, DispatchStatus::Cancelled);
}

#[tokio::test]
async fn store_rejects_failed_compare_and_set_without_partial_output() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();

    let turn_error = fixture
        .store
        .transition_turn("turn-1", TurnStatus::Running, TurnStatus::Completed, None)
        .await
        .unwrap_err();
    assert!(matches!(
        turn_error,
        SchedulerStoreError::TransitionConflict { .. }
    ));
    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();

    fixture
        .store
        .queue_dispatch(fixture.dispatch("dispatch-1", "turn-1", &fixture.agent_a))
        .await
        .unwrap();
    fixture.store.start_dispatch("dispatch-1").await.unwrap();
    let dispatch_error = fixture
        .store
        .start_dispatch("dispatch-1")
        .await
        .unwrap_err();
    assert!(matches!(
        dispatch_error,
        SchedulerStoreError::TransitionConflict { .. }
    ));

    let stream_id = Uuid::new_v4();
    let event_id = format!("{stream_id}:1");
    sqlx::query(
        "INSERT INTO stream_events \
         (id, stream_id, thread_id, seq, event_id, kind, payload_json, created_at) \
         VALUES ('existing-event', ?, ?, 1, ?, 'agent_message', '{}', ?)",
    )
    .bind(stream_id.to_string())
    .bind(&fixture.thread_id)
    .bind(&event_id)
    .bind(NOW)
    .execute(&fixture.pool)
    .await
    .unwrap();

    let error = fixture
        .store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-1".to_owned(),
            next: DispatchStatus::Completed,
            artifact: Some(json!({"must": "roll back"})),
            total_tokens: 99,
            failure_code: None,
            output: Some(DispatchOutput {
                thread_id: fixture.thread_id.clone(),
                group_id: fixture.group_id.clone(),
                message: NewMessage {
                    id: "orphan-message".to_owned(),
                    sender_type: "agent".to_owned(),
                    sender_id: Some(fixture.agent_a.clone()),
                    message_type: "text".to_owned(),
                    content: "Must roll back".to_owned(),
                    content_json: None,
                },
                event: StreamEvent::new(
                    stream_id,
                    1,
                    StreamEventKind::AgentMessage,
                    json!({"message_id": "orphan-message"}),
                ),
            }),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(error, SchedulerStoreError::Persistence(_)),
        "unexpected error: {error:?}"
    );

    let orphan_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = 'orphan-message'")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    let dispatch_after_failure: (String, Option<String>, i64, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT status, artifact_json, total_tokens, completed_at, output_message_id \
             FROM agent_dispatches WHERE id = 'dispatch-1'",
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(orphan_count, 0);
    assert_eq!(dispatch_after_failure.0, "running");
    assert_eq!(dispatch_after_failure.1, None);
    assert_eq!(dispatch_after_failure.2, 0);
    assert_eq!(dispatch_after_failure.3, None);
    assert_eq!(dispatch_after_failure.4, None);
    assert_eq!(event_count, 1);
    let next_seq: i64 = sqlx::query_scalar("SELECT next_seq FROM threads WHERE id = ?")
        .bind(&fixture.thread_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(next_seq, 1);
}

#[tokio::test]
async fn store_rechecks_turn_before_visible_completion() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();
    fixture
        .store
        .queue_dispatch(fixture.dispatch("dispatch-1", "turn-1", &fixture.agent_a))
        .await
        .unwrap();
    fixture.store.start_dispatch("dispatch-1").await.unwrap();

    sqlx::query("UPDATE group_turns SET status = 'superseded' WHERE id = 'turn-1'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    let error = fixture
        .store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-1".to_owned(),
            next: DispatchStatus::Completed,
            artifact: None,
            total_tokens: 1,
            failure_code: None,
            output: Some(DispatchOutput {
                thread_id: fixture.thread_id.clone(),
                group_id: fixture.group_id.clone(),
                message: NewMessage {
                    id: "stale-message".to_owned(),
                    sender_type: "agent".to_owned(),
                    sender_id: Some(fixture.agent_a.clone()),
                    message_type: "text".to_owned(),
                    content: "Stale".to_owned(),
                    content_json: None,
                },
                event: StreamEvent::new(
                    Uuid::new_v4(),
                    1,
                    StreamEventKind::AgentMessage,
                    json!({"message_id": "stale-message"}),
                ),
            }),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SchedulerStoreError::InactiveTurn {
            status: TurnStatus::Superseded,
            ..
        }
    ));
    let message_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = 'stale-message'")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(message_count, 0);
    assert_eq!(event_count, 0);
    let dispatch_status: (String, Option<String>) = sqlx::query_as(
        "SELECT status, output_message_id FROM agent_dispatches WHERE id = 'dispatch-1'",
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(dispatch_status.0, "running");
    assert_eq!(dispatch_status.1, None);
}

#[tokio::test]
async fn store_rejects_mismatched_visible_event_linkage() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();
    fixture
        .store
        .queue_dispatch(fixture.dispatch("dispatch-1", "turn-1", &fixture.agent_a))
        .await
        .unwrap();
    fixture.store.start_dispatch("dispatch-1").await.unwrap();

    let wrong_kind_error = fixture
        .store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-1".to_owned(),
            next: DispatchStatus::Completed,
            artifact: None,
            total_tokens: 1,
            failure_code: None,
            output: Some(DispatchOutput {
                thread_id: fixture.thread_id.clone(),
                group_id: fixture.group_id.clone(),
                message: NewMessage {
                    id: "message-1".to_owned(),
                    sender_type: "agent".to_owned(),
                    sender_id: Some(fixture.agent_a.clone()),
                    message_type: "text".to_owned(),
                    content: "Finished".to_owned(),
                    content_json: None,
                },
                event: StreamEvent::new(
                    Uuid::new_v4(),
                    1,
                    StreamEventKind::Token,
                    json!({"message_id": "message-1"}),
                ),
            }),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        wrong_kind_error,
        SchedulerStoreError::InvalidInput(_)
    ));

    let error = fixture
        .store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-1".to_owned(),
            next: DispatchStatus::Completed,
            artifact: None,
            total_tokens: 1,
            failure_code: None,
            output: Some(DispatchOutput {
                thread_id: fixture.thread_id.clone(),
                group_id: fixture.group_id.clone(),
                message: NewMessage {
                    id: "message-1".to_owned(),
                    sender_type: "agent".to_owned(),
                    sender_id: Some(fixture.agent_a.clone()),
                    message_type: "text".to_owned(),
                    content: "Finished".to_owned(),
                    content_json: None,
                },
                event: StreamEvent::new(
                    Uuid::new_v4(),
                    1,
                    StreamEventKind::AgentMessage,
                    json!({"message_id": "different-message"}),
                ),
            }),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, SchedulerStoreError::InvalidInput(_)));
    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(message_count, 0);
}

#[tokio::test]
async fn store_superseding_turn_interrupts_active_dispatches() {
    let fixture = Fixture::new().await;
    fixture
        .store
        .create_turn(fixture.turn("turn-1"))
        .await
        .unwrap();
    fixture
        .store
        .transition_turn("turn-1", TurnStatus::Pending, TurnStatus::Running, None)
        .await
        .unwrap();
    for (id, target) in [
        ("dispatch-running", &fixture.agent_a),
        ("dispatch-queued", &fixture.agent_b),
    ] {
        fixture
            .store
            .queue_dispatch(fixture.dispatch(id, "turn-1", target))
            .await
            .unwrap();
    }
    fixture
        .store
        .start_dispatch("dispatch-running")
        .await
        .unwrap();

    fixture
        .store
        .transition_turn(
            "turn-1",
            TurnStatus::Running,
            TurnStatus::Superseded,
            Some("superseded"),
        )
        .await
        .unwrap();
    let trace = fixture.store.load_turn_trace("turn-1").await.unwrap();
    assert_eq!(trace.turn.status, TurnStatus::Superseded);
    assert_eq!(trace.turn.termination_reason, Some(TurnReason::Superseded));
    assert_eq!(trace.dispatches[0].status, DispatchStatus::Interrupted);
    assert_eq!(trace.dispatches[1].status, DispatchStatus::Cancelled);
}
