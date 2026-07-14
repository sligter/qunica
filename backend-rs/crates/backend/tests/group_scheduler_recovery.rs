use std::{path::Path, sync::Arc};

use ag_swarmer_backend::{
    db::Db,
    runtime::group_scheduler::{
        ActionKind, DispatchStatus, FinishDispatch, NewDispatch, NewTurn, SchedulerStore,
        SelectionReason, TurnReason, TurnStatus,
    },
    server::{build_state, ServerConfig},
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::Mutex;

const NOW: &str = "2026-07-14T00:00:00Z";

#[tokio::test]
async fn startup_recovery_finalizes_incomplete_scheduler_state_without_output() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("scheduler.sqlite3"));
    let db = Db::connect(&database_url).await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool().clone();
    seed_scheduler_entities(&pool).await;

    let store = SchedulerStore::new(pool.clone(), Arc::new(Mutex::new(())));
    store
        .create_turn(new_turn("turn-pending", "thread-pending"))
        .await
        .unwrap();

    store
        .create_turn(new_turn("turn-running", "thread-running"))
        .await
        .unwrap();
    store
        .transition_turn(
            "turn-running",
            TurnStatus::Pending,
            TurnStatus::Running,
            None,
        )
        .await
        .unwrap();
    store
        .queue_dispatch(new_dispatch("dispatch-running", "turn-running"))
        .await
        .unwrap();
    store.start_dispatch("dispatch-running").await.unwrap();
    store
        .queue_dispatch(new_dispatch("dispatch-queued", "turn-running"))
        .await
        .unwrap();

    store
        .create_turn(new_turn("turn-waiting", "thread-waiting"))
        .await
        .unwrap();
    store
        .transition_turn(
            "turn-waiting",
            TurnStatus::Pending,
            TurnStatus::Running,
            None,
        )
        .await
        .unwrap();
    store
        .queue_dispatch(new_dispatch("dispatch-waiting", "turn-waiting"))
        .await
        .unwrap();
    store.start_dispatch("dispatch-waiting").await.unwrap();
    store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-waiting".to_owned(),
            next: DispatchStatus::WaitingForUser,
            artifact: None,
            total_tokens: 0,
            failure_code: None,
            output: None,
        })
        .await
        .unwrap();
    store
        .transition_turn(
            "turn-waiting",
            TurnStatus::Running,
            TurnStatus::WaitingForUser,
            Some(TurnReason::WaitingForUser.as_str()),
        )
        .await
        .unwrap();

    store
        .create_turn(new_turn("turn-terminal", "thread-terminal"))
        .await
        .unwrap();
    store
        .transition_turn(
            "turn-terminal",
            TurnStatus::Pending,
            TurnStatus::Running,
            None,
        )
        .await
        .unwrap();
    store
        .queue_dispatch(new_dispatch("dispatch-terminal", "turn-terminal"))
        .await
        .unwrap();
    store
        .transition_turn(
            "turn-terminal",
            TurnStatus::Running,
            TurnStatus::Cancelled,
            Some(TurnReason::UserCancelled.as_str()),
        )
        .await
        .unwrap();

    let messages_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    drop(store);
    drop(pool);
    drop(db);

    let state = build_state(&ServerConfig {
        host: "127.0.0.1".to_owned(),
        port: 0,
        database_url,
        secret_key: "test-secret".to_owned(),
        access_token_expire_minutes: 60,
        app_data_dir: Some(directory.path().join("app-data")),
    })
    .await
    .unwrap();
    let recovered = SchedulerStore::new(state.db.pool().clone(), state.write_lock.clone());

    let pending = recovered.load_turn_trace("turn-pending").await.unwrap();
    assert_eq!(pending.turn.status, TurnStatus::Failed);
    assert_eq!(
        pending.turn.termination_reason,
        Some(TurnReason::ServerRestart)
    );
    assert!(pending.turn.completed_at.is_some());

    let running = recovered.load_turn_trace("turn-running").await.unwrap();
    assert_eq!(running.turn.status, TurnStatus::Failed);
    assert_eq!(
        running.turn.termination_reason,
        Some(TurnReason::ServerRestart)
    );
    assert_eq!(running.dispatches[0].status, DispatchStatus::Interrupted);
    assert_eq!(running.dispatches[1].status, DispatchStatus::Cancelled);

    let waiting = recovered.load_turn_trace("turn-waiting").await.unwrap();
    assert_eq!(waiting.turn.status, TurnStatus::Failed);
    assert_eq!(
        waiting.turn.termination_reason,
        Some(TurnReason::ServerRestart)
    );
    assert_eq!(waiting.dispatches[0].status, DispatchStatus::WaitingForUser);

    let terminal = recovered.load_turn_trace("turn-terminal").await.unwrap();
    assert_eq!(terminal.turn.status, TurnStatus::Cancelled);
    assert_eq!(
        terminal.turn.termination_reason,
        Some(TurnReason::UserCancelled)
    );
    assert_eq!(terminal.dispatches[0].status, DispatchStatus::Cancelled);

    let messages_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(messages_after, messages_before);
    assert_eq!(events_after, events_before);

    let traces_after_startup = vec![pending, running, waiting, terminal];
    recovered.recover_incomplete_turns().await.unwrap();
    for expected in traces_after_startup {
        assert_eq!(
            recovered.load_turn_trace(&expected.turn.id).await.unwrap(),
            expected
        );
    }
}

async fn seed_scheduler_entities(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, created_at, updated_at) \
         VALUES ('user-1', 'owner@example.test', 'hash', 'Owner', ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO groups (id, owner_id, name, created_at, updated_at) \
         VALUES ('group-1', 'user-1', 'Team', ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
    for thread_id in [
        "thread-pending",
        "thread-running",
        "thread-waiting",
        "thread-terminal",
    ] {
        sqlx::query(
            "INSERT INTO threads (id, group_id, created_at, updated_at) \
             VALUES (?, 'group-1', ?, ?)",
        )
        .bind(thread_id)
        .bind(NOW)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO agents (id, owner_id, name, system_prompt, created_at, updated_at) \
         VALUES ('agent-1', 'user-1', 'Agent', 'Help', ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

fn new_turn(id: &str, thread_id: &str) -> NewTurn {
    NewTurn {
        id: id.to_owned(),
        thread_id: thread_id.to_owned(),
        group_id: "group-1".to_owned(),
        trigger_message_id: None,
        scheduler_strategy: "deterministic".to_owned(),
        config_snapshot: json!({"max_agent_steps": 8}),
        topology_snapshot: json!({"mode": "mesh"}),
    }
}

fn new_dispatch(id: &str, turn_id: &str) -> NewDispatch {
    NewDispatch {
        id: id.to_owned(),
        turn_id: turn_id.to_owned(),
        parent_dispatch_id: None,
        source_agent_id: None,
        target_agent_id: "agent-1".to_owned(),
        selection_reason: SelectionReason::DeterministicOrder,
        action_kind: ActionKind::Speak,
        hop: 0,
        input_message_id: None,
    }
}

fn sqlite_url(path: &Path) -> String {
    let mut display = path.to_string_lossy().replace('\\', "/");
    if !display.starts_with('/') && !display.starts_with("//") {
        display = format!("/{display}");
    }
    format!("sqlite://{display}?mode=rwc")
}
