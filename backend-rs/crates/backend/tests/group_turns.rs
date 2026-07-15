use ag_swarmer_backend::{
    api::{router_with_state_for_tests, AppState},
    runtime::group_scheduler::{
        ActionKind, DispatchStatus, FinishDispatch, NewDispatch, NewTurn, SchedulerStore,
        SelectionReason, TurnReason, TurnStatus,
    },
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const NOW: &str = "2026-07-14T00:00:00Z";

#[tokio::test]
async fn trace_is_owner_scoped_orders_dispatches_and_filters_private_artifacts() {
    let (app, state) = router_with_state_for_tests().await;
    let owner_token = register_and_login(&app, "turn-owner@example.test").await;
    let other_token = register_and_login(&app, "turn-other@example.test").await;
    let owner_id = owner_id(&state, "turn-owner@example.test").await;
    let fixture = TurnFixture::seed(&state, &owner_id).await;
    let store = scheduler_store(&state);

    store.create_turn(fixture.new_turn(None)).await.unwrap();
    store
        .transition_turn(
            &fixture.turn_id,
            TurnStatus::Pending,
            TurnStatus::Running,
            None,
        )
        .await
        .unwrap();
    store
        .queue_dispatch(fixture.new_dispatch("dispatch-one"))
        .await
        .unwrap();
    store.start_dispatch("dispatch-one").await.unwrap();
    store
        .finish_dispatch(FinishDispatch {
            dispatch_id: "dispatch-one".to_owned(),
            next: DispatchStatus::Completed,
            artifact: Some(json!({
                "mode": "handoff",
                "target_agent_id": fixture.agent_id,
                "child_dispatch_id": "child-1",
                "final_content": "private helper response",
                "reasoning": ["hidden"],
                "tool_io": {"secret": "hidden"},
            })),
            total_tokens: 12,
            failure_code: None,
            output: None,
        })
        .await
        .unwrap();
    store
        .transition_turn(
            &fixture.turn_id,
            TurnStatus::Running,
            TurnStatus::Completed,
            Some(TurnReason::Silence.as_str()),
        )
        .await
        .unwrap();

    let uri = format!(
        "/api/v2/groups/{}/turns/{}",
        fixture.group_id, fixture.turn_id
    );
    let (status, body) = send(&app, authed("GET", &uri, &owner_token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["turn"]["id"], fixture.turn_id);
    assert_eq!(body["dispatches"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["dispatches"][0]["artifact"],
        json!({
            "mode": "handoff",
            "target_agent_id": fixture.agent_id,
            "child_dispatch_id": "child-1",
        })
    );
    assert!(body["dispatches"][0]["artifact"]
        .get("final_content")
        .is_none());
    assert!(body["dispatches"][0]["artifact"].get("reasoning").is_none());
    assert_eq!(body["estimated_cost"], Value::Null);
    assert_eq!(body["cost_estimation_status"], "unavailable");

    let (status, _) = send(&app, authed("GET", &uri, &other_token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn cancellation_is_idempotent_and_stops_the_matching_active_turn() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "turn-cancel@example.test").await;
    let owner_id = owner_id(&state, "turn-cancel@example.test").await;
    let fixture = TurnFixture::seed(&state, &owner_id).await;
    let store = scheduler_store(&state);

    store.create_turn(fixture.new_turn(None)).await.unwrap();
    store
        .transition_turn(
            &fixture.turn_id,
            TurnStatus::Pending,
            TurnStatus::Running,
            None,
        )
        .await
        .unwrap();
    store
        .queue_dispatch(fixture.new_dispatch("dispatch-running"))
        .await
        .unwrap();
    store.start_dispatch("dispatch-running").await.unwrap();
    let active = state
        .active_turns
        .register(fixture.thread_id.clone(), fixture.turn_id.clone())
        .await;

    let uri = format!(
        "/api/v2/groups/{}/turns/{}/cancel",
        fixture.group_id, fixture.turn_id
    );
    let (status, first) = send(&app, authed("POST", &uri, &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["turn"]["status"], "cancelled");
    assert_eq!(first["turn"]["termination_reason"], "user_cancelled");
    assert_eq!(first["dispatches"][0]["status"], "interrupted");
    assert!(active.cancellation.is_cancelled());

    let (status, second) = send(&app, authed("POST", &uri, &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["turn"]["status"], "cancelled");
    assert_eq!(second["dispatches"][0]["status"], "interrupted");
}

#[tokio::test]
async fn trigger_message_history_contains_scheduler_causality_and_summary() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "turn-history@example.test").await;
    let owner_id = owner_id(&state, "turn-history@example.test").await;
    let fixture = TurnFixture::seed(&state, &owner_id).await;
    let trigger_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO messages \
         (id, thread_id, group_id, seq, sender_type, sender_id, message_type, content, status, created_at) \
         VALUES (?, ?, ?, 1, 'user', ?, 'text', 'Start a bounded turn', 'visible', ?)",
    )
    .bind(&trigger_id)
    .bind(&fixture.thread_id)
    .bind(&fixture.group_id)
    .bind(&owner_id)
    .bind(NOW)
    .execute(state.db.pool())
    .await
    .unwrap();

    let store = scheduler_store(&state);
    store
        .create_turn(fixture.new_turn(Some(trigger_id.clone())))
        .await
        .unwrap();
    store
        .transition_turn(
            &fixture.turn_id,
            TurnStatus::Pending,
            TurnStatus::Running,
            None,
        )
        .await
        .unwrap();
    store
        .transition_turn(
            &fixture.turn_id,
            TurnStatus::Running,
            TurnStatus::Completed,
            Some(TurnReason::Silence.as_str()),
        )
        .await
        .unwrap();

    let uri = format!("/api/v2/groups/{}/messages", fixture.group_id);
    let (status, messages) = send(&app, authed("GET", &uri, &token)).await;
    assert_eq!(status, StatusCode::OK);
    let trigger = messages
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["id"] == trigger_id)
        .unwrap();
    assert_eq!(trigger["turn_id"], fixture.turn_id);
    assert_eq!(trigger["dispatch_id"], Value::Null);
    assert_eq!(trigger["reply_to_message_id"], Value::Null);
    assert_eq!(trigger["turn_summary"]["status"], "completed");
    assert_eq!(trigger["turn_summary"]["termination_reason"], "silence");
}

struct TurnFixture {
    group_id: String,
    thread_id: String,
    agent_id: String,
    turn_id: String,
}

impl TurnFixture {
    async fn seed(state: &AppState, owner_id: &str) -> Self {
        let group_id = Uuid::new_v4().to_string();
        let thread_id = Uuid::new_v4().to_string();
        let agent_id = Uuid::new_v4().to_string();
        let turn_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO groups (id, owner_id, name, created_at, updated_at) VALUES (?, ?, 'Turn test', ?, ?)",
        )
        .bind(&group_id)
        .bind(owner_id)
        .bind(NOW)
        .bind(NOW)
        .execute(state.db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO threads (id, group_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&thread_id)
        .bind(&group_id)
        .bind(NOW)
        .bind(NOW)
        .execute(state.db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agents (id, owner_id, name, system_prompt, created_at, updated_at) \
             VALUES (?, ?, 'Turn Agent', 'Help', ?, ?)",
        )
        .bind(&agent_id)
        .bind(owner_id)
        .bind(NOW)
        .bind(NOW)
        .execute(state.db.pool())
        .await
        .unwrap();
        Self {
            group_id,
            thread_id,
            agent_id,
            turn_id,
        }
    }

    fn new_turn(&self, trigger_message_id: Option<String>) -> NewTurn {
        NewTurn {
            id: self.turn_id.clone(),
            thread_id: self.thread_id.clone(),
            group_id: self.group_id.clone(),
            trigger_message_id,
            scheduler_strategy: "deterministic".to_owned(),
            config_snapshot: json!({"max_agent_steps": 8}),
            topology_snapshot: json!({"mode": "mesh"}),
        }
    }

    fn new_dispatch(&self, id: &str) -> NewDispatch {
        NewDispatch {
            id: id.to_owned(),
            turn_id: self.turn_id.clone(),
            parent_dispatch_id: None,
            source_agent_id: None,
            target_agent_id: self.agent_id.clone(),
            selection_reason: SelectionReason::DeterministicOrder,
            action_kind: ActionKind::Speak,
            hop: 0,
            input_message_id: None,
        }
    }
}

fn scheduler_store(state: &AppState) -> SchedulerStore {
    SchedulerStore::new(state.db.pool().clone(), state.write_lock.clone())
}

async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        json_request(
            "POST",
            "/api/v2/auth/register",
            None,
            json!({"email": email, "password": "supersecret", "name": "Turn Tester"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = send(
        app,
        json_request(
            "POST",
            "/api/v2/auth/login",
            None,
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["access_token"].as_str().unwrap().to_owned()
}

async fn owner_id(state: &AppState, email: &str) -> String {
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn json_request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}
