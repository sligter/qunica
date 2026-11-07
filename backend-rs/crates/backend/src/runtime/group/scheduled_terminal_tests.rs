use super::{
    cancel_scheduled_turn, emit_turn_terminal, fail_scheduled_persistence, BudgetLimits,
    SequenceAllocator, StepErr, StreamCtx, StreamEvent, StreamEventKind, TurnBudget, TurnOutcome,
    TurnReason, TurnStatus,
};
use crate::{
    db::Db,
    runtime::group_scheduler::{NewTurn, SchedulerStore},
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

struct Fixture {
    pool: SqlitePool,
    store: SchedulerStore,
    ctx: StreamCtx,
    rx: mpsc::Receiver<StreamEvent<Value>>,
}

impl Fixture {
    async fn new() -> Self {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        let pool = db.pool().clone();
        for sql in [
            "INSERT INTO users(id,email,password_hash,name,created_at,updated_at) VALUES('owner','terminal@example.test','hash','Owner','now','now')",
            "INSERT INTO groups(id,owner_id,name,created_at,updated_at) VALUES('group','owner','Team','now','now')",
            "INSERT INTO threads(id,group_id,created_at,updated_at) VALUES('thread','group','now','now')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let lock = Arc::new(Mutex::new(()));
        let store = SchedulerStore::new(pool.clone(), lock.clone());
        store
            .create_turn(NewTurn {
                id: "turn".into(),
                thread_id: "thread".into(),
                group_id: "group".into(),
                trigger_message_id: None,
                scheduler_strategy: "deterministic".into(),
                config_snapshot: json!({}),
                topology_snapshot: json!({"mode":"mesh"}),
            })
            .await
            .unwrap();
        store
            .transition_turn("turn", TurnStatus::Pending, TurnStatus::Running, None)
            .await
            .unwrap();
        let (tx, rx) = mpsc::channel(32);
        let ctx = StreamCtx {
            stream_id: Uuid::new_v4(),
            seq: 0,
            tx,
            allocator: SequenceAllocator::new(pool.clone(), lock),
            thread_id: "thread".into(),
            group_id: "group".into(),
            model_override: None,
            effort_override: None,
            scheduled_dispatch: None,
            scheduled_total_tokens: 0,
            scheduled_accounted_tokens: 0,
            private_execution: false,
            turn_cancellation: None,
            active_turn: None,
            resume: None,
            cancellation: None,
            decision: None,
            latest_request: None,
            pending: None,
        };
        Self {
            pool,
            store,
            ctx,
            rx,
        }
    }

    async fn sql(&self, sql: &str) {
        sqlx::query(sql).execute(&self.pool).await.unwrap();
    }

    async fn status(&self) -> (String, Option<String>) {
        sqlx::query_as("SELECT status,termination_reason FROM group_turns WHERE id='turn'")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn events(&mut self) -> Vec<(String, Value, i64)> {
        let rows: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT kind,payload_json,seq FROM stream_events ORDER BY seq")
                .fetch_all(&self.pool)
                .await
                .unwrap();
        let mut delivered = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            delivered.push(event);
        }
        assert_eq!(rows.len(), delivered.len());
        for ((kind, payload, seq), event) in rows.iter().zip(&delivered) {
            assert_eq!(json!(kind), serde_json::to_value(&event.kind).unwrap());
            assert_eq!(*seq, event.seq);
            assert_eq!(*payload, serde_json::to_string(&event.payload).unwrap());
            assert_eq!(event.stream_id, self.ctx.stream_id);
            assert_eq!(event.event_id, format!("{}:{}", event.stream_id, event.seq));
        }
        rows.into_iter()
            .map(|(kind, payload, seq)| (kind, serde_json::from_str(&payload).unwrap(), seq))
            .collect()
    }
}

const FINAL_STATUSES: [TurnStatus; 7] = [
    TurnStatus::Completed,
    TurnStatus::Silence,
    TurnStatus::BudgetExhausted,
    TurnStatus::FailureBudgetExhausted,
    TurnStatus::Cancelled,
    TurnStatus::Superseded,
    TurnStatus::Failed,
];

#[tokio::test]
async fn terminal_mapping_and_payload_preserve_status_reason_budget_and_done() {
    for status in FINAL_STATUSES
        .into_iter()
        .chain([TurnStatus::WaitingForUser])
    {
        let mut f = Fixture::new().await;
        let mut budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
        budget.record_dispatch("a");
        budget.record_tokens(123);
        assert!(emit_turn_terminal(
            &mut f.ctx,
            "turn",
            status,
            Some(TurnReason::Silence),
            &budget
        )
        .await
        .is_ok());
        let rows = f.events().await;
        let kind = match status {
            TurnStatus::BudgetExhausted | TurnStatus::FailureBudgetExhausted => {
                "turn_budget_exhausted"
            }
            TurnStatus::Cancelled => "turn_cancelled",
            TurnStatus::Superseded => "turn_superseded",
            _ => "turn_completed",
        };
        assert_eq!(
            rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
            [kind, "done"]
        );
        assert_eq!(
            rows[0].1,
            json!({
                "turn_id":"turn", "status":status.as_str(), "reason":"silence",
                "budget": {"agent_steps":1, "moderator_calls":0, "consecutive_failures":0,
                    "total_failures":0, "total_tokens":123,
                    "limits": {"unbounded":false, "max_agent_steps":8,
                        "max_steps_per_agent":3, "max_hops":5, "max_moderator_calls":4,
                        "max_consecutive_failures":3, "max_total_failures":6, "max_total_tokens":120000}}
            })
        );
        assert_eq!(rows[1].1, json!({"turn_id":"turn"}));
        assert_eq!((rows[0].2, rows[1].2), (0, 1));
    }
}

#[tokio::test]
async fn nonterminal_status_is_rejected_without_allocating_or_delivering_events() {
    for status in [TurnStatus::Queued, TurnStatus::Pending, TurnStatus::Running] {
        let mut f = Fixture::new().await;
        let budget = TurnBudget::new(BudgetLimits::with_auto_steps(1, Some(8)));
        assert!(matches!(
            emit_turn_terminal(&mut f.ctx, "turn", status, None, &budget).await,
            Err(StepErr::SchedulerPersistence)
        ));
        assert!(f.events().await.is_empty());
        assert_eq!(f.ctx.seq, 0);
    }
}

#[tokio::test]
async fn terminal_pair_flushes_pending_output_first() {
    let mut f = Fixture::new().await;
    assert!(f
        .ctx
        .emit_coalesced(StreamEventKind::Token, json!({"text":"held"}))
        .await
        .is_ok());
    let budget = TurnBudget::new(BudgetLimits::with_auto_steps(1, Some(8)));
    assert!(
        emit_turn_terminal(&mut f.ctx, "turn", TurnStatus::Completed, None, &budget)
            .await
            .is_ok()
    );
    let rows = f.events().await;
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["token", "turn_completed", "done"]
    );
    assert_eq!(rows[0].1, json!({"text":"held"}));
}

#[tokio::test]
async fn failed_done_insert_rolls_back_the_pair_and_preserves_committed_status() {
    let mut f = Fixture::new().await;
    f.store
        .transition_turn("turn", TurnStatus::Running, TurnStatus::Completed, None)
        .await
        .unwrap();
    f.sql("CREATE TRIGGER reject_done BEFORE INSERT ON stream_events WHEN NEW.kind='done' BEGIN SELECT RAISE(ABORT,'fixture done failure'); END").await;
    let budget = TurnBudget::new(BudgetLimits::with_auto_steps(1, Some(8)));
    assert!(matches!(
        emit_turn_terminal(&mut f.ctx, "turn", TurnStatus::Completed, None, &budget).await,
        Err(StepErr::SchedulerPersistence)
    ));
    assert!(f.events().await.is_empty());
    assert_eq!(f.ctx.seq, 2);
    f.sql("DROP TRIGGER reject_done").await;
    assert!(matches!(
        fail_scheduled_persistence(&mut f.ctx, &f.store, "turn").await,
        Ok(TurnOutcome::Error)
    ));
    assert_eq!(f.status().await, ("completed".into(), None));
    let rows = f.events().await;
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["error", "done"]
    );
    assert_eq!((rows[0].2, rows[1].2), (2, 3));
}

#[tokio::test]
async fn disconnected_receiver_does_not_undo_the_durable_pair() {
    let mut f = Fixture::new().await;
    f.rx.close();
    let budget = TurnBudget::new(BudgetLimits::with_auto_steps(1, Some(8)));
    assert!(
        emit_turn_terminal(&mut f.ctx, "turn", TurnStatus::Completed, None, &budget)
            .await
            .is_ok()
    );
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM stream_events ORDER BY seq")
        .fetch_all(&f.pool)
        .await
        .unwrap();
    assert_eq!(kinds, ["turn_completed", "done"]);
}

#[tokio::test]
async fn cancellation_uses_the_persisted_budget_and_bypasses_the_cancelled_token() {
    let mut f = Fixture::new().await;
    f.store
        .update_turn_budget("turn", 2, 1, 1, 4, 123)
        .await
        .unwrap();
    f.ctx.scheduled_total_tokens = 999;
    f.ctx.cancellation = Some(Arc::new(AtomicBool::new(true)));
    assert!(matches!(
        cancel_scheduled_turn(&mut f.ctx, &f.store, "turn").await,
        Ok(TurnOutcome::Cancelled)
    ));
    assert_eq!(
        f.status().await,
        ("cancelled".into(), Some("user_cancelled".into()))
    );
    let rows = f.events().await;
    assert_eq!(rows[0].0, "turn_cancelled");
    assert_eq!(
        rows[0].1["budget"],
        json!({"agent_steps":2,"moderator_calls":1,"consecutive_failures":1,"total_failures":4,"total_tokens":123})
    );
    assert_eq!(rows[1].0, "done");
}

#[tokio::test]
async fn cancellation_preserves_existing_terminal_status_including_superseded_and_budget() {
    for status in FINAL_STATUSES {
        let mut f = Fixture::new().await;
        f.store
            .transition_turn("turn", TurnStatus::Running, status, None)
            .await
            .unwrap();
        assert!(matches!(
            cancel_scheduled_turn(&mut f.ctx, &f.store, "turn").await,
            Ok(TurnOutcome::Cancelled)
        ));
        assert_eq!(f.status().await, (status.as_str().into(), None));
        let rows = f.events().await;
        let kind = match status {
            TurnStatus::Cancelled => "turn_cancelled",
            TurnStatus::Superseded => "turn_superseded",
            _ => "turn_completed",
        };
        assert_eq!(rows[0].0, kind);
        assert_eq!(rows[0].1["status"], status.as_str());
        assert_eq!(rows[1].0, "done");
    }
}

#[tokio::test]
async fn persistence_failure_commits_failed_status_before_error_delivery() {
    let mut f = Fixture::new().await;
    f.sql("CREATE TRIGGER require_failed BEFORE INSERT ON stream_events WHEN (SELECT status FROM group_turns WHERE id='turn')!='failed' BEGIN SELECT RAISE(ABORT,'state must commit first'); END").await;
    assert!(matches!(
        fail_scheduled_persistence(&mut f.ctx, &f.store, "turn").await,
        Ok(TurnOutcome::Error)
    ));
    assert_eq!(
        f.status().await,
        ("failed".into(), Some("persistence_failed".into()))
    );
    let rows = f.events().await;
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["error", "done"]
    );
    assert_eq!(rows[0].1, json!({"message":"scheduler persistence failed"}));
}

#[tokio::test]
async fn persistence_failure_does_not_replace_any_existing_terminal_status() {
    for status in FINAL_STATUSES {
        let mut f = Fixture::new().await;
        f.store
            .transition_turn("turn", TurnStatus::Running, status, None)
            .await
            .unwrap();
        assert!(matches!(
            fail_scheduled_persistence(&mut f.ctx, &f.store, "turn").await,
            Ok(TurnOutcome::Error)
        ));
        assert_eq!(f.status().await, (status.as_str().into(), None));
        assert_eq!(f.events().await.len(), 2);
    }
}

#[tokio::test]
async fn failed_state_write_is_best_effort_without_faking_a_commit() {
    let mut f = Fixture::new().await;
    f.sql("CREATE TRIGGER reject_failed BEFORE UPDATE ON group_turns WHEN NEW.status='failed' BEGIN SELECT RAISE(ABORT,'fixture state failure'); END").await;
    assert!(matches!(
        fail_scheduled_persistence(&mut f.ctx, &f.store, "turn").await,
        Ok(TurnOutcome::Error)
    ));
    assert_eq!(f.status().await, ("running".into(), None));
    assert_eq!(f.events().await.len(), 2);
}

#[tokio::test]
async fn failure_cleanup_respects_cancellation_after_committing_failed_state() {
    let mut f = Fixture::new().await;
    f.ctx.cancellation = Some(Arc::new(AtomicBool::new(true)));
    assert!(fail_scheduled_persistence(&mut f.ctx, &f.store, "turn")
        .await
        .is_err());
    assert_eq!(
        f.status().await,
        ("failed".into(), Some("persistence_failed".into()))
    );
    assert!(f.events().await.is_empty());
}

#[tokio::test]
async fn failed_cancel_write_enters_the_existing_persistence_failure_path() {
    let mut f = Fixture::new().await;
    f.sql("CREATE TRIGGER reject_cancel BEFORE UPDATE ON group_turns WHEN NEW.status='cancelled' BEGIN SELECT RAISE(ABORT,'fixture cancel failure'); END").await;
    assert!(matches!(
        cancel_scheduled_turn(&mut f.ctx, &f.store, "turn").await,
        Ok(TurnOutcome::Error)
    ));
    assert_eq!(
        f.status().await,
        ("failed".into(), Some("persistence_failed".into()))
    );
    let rows = f.events().await;
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["error", "done"]
    );
}

#[tokio::test]
async fn unreadable_trace_does_not_invent_a_status_or_retry_execution() {
    let mut f = Fixture::new().await;
    f.sql("UPDATE group_turns SET config_snapshot_json='invalid json' WHERE id='turn'")
        .await;
    assert!(matches!(
        fail_scheduled_persistence(&mut f.ctx, &f.store, "turn").await,
        Ok(TurnOutcome::Error)
    ));
    assert_eq!(f.status().await, ("running".into(), None));
    assert_eq!(f.events().await.len(), 2);
}
