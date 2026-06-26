//! Group streaming runtime integration tests.
//!
//! Each test seeds users/groups via the public API and seeds agents, group
//! bindings and LLM providers directly through the shared pool (there is no
//! group-agent or provider binding API yet). LLM streaming is exercised against
//! a local fake HTTP server that replays canned OpenAI-style SSE; no live
//! external API is contacted.

use ag_swarmer_backend::{
    api::{router_with_state_for_tests, AppState},
    runtime::{run_group_turn, RuntimeServices, StreamEventKind, TurnOutcome, TurnRequest},
};
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::IntoResponse,
    Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

/// Drive the SSE endpoint and return the parsed `StreamEvent` JSON frames.
async fn stream_events(app: &Router, uri: &str, token: &str, body: Value) -> Vec<Value> {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    text.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| serde_json::from_str::<Value>(data.trim()).unwrap())
        .collect()
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn authed_json(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": email, "password": "supersecret", "name": "Tester"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, token) = send(
        app,
        post_json(
            "/api/v2/auth/login",
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    token["access_token"].as_str().unwrap().to_string()
}

async fn create_workspace(app: &Router, token: &str) -> String {
    let (status, workspace) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            token,
            json!({"name": "WS", "backend_type": "cloud_sandbox"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    workspace["id"].as_str().unwrap().to_string()
}

async fn create_group(app: &Router, token: &str, workspace_id: &str, flags: Value) -> String {
    let mut body = json!({"name": "Team", "workspace_id": workspace_id});
    if let (Some(obj), Some(extra)) = (body.as_object_mut(), flags.as_object()) {
        for (key, value) in extra {
            obj.insert(key.clone(), value.clone());
        }
    }
    let (status, group) = send(app, authed_json("POST", "/api/v2/groups", token, body)).await;
    assert_eq!(status, StatusCode::CREATED);
    group["id"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// Direct-pool seeding (no binding API yet)
// ---------------------------------------------------------------------------

async fn owner_id(state: &AppState, email: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
}

async fn seed_provider(state: &AppState, owner_id: &str, base_url: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO llm_providers \
         (id, owner_id, name, kind, base_url, api_key, default_model, reasoning_passback, \
          status, created_at, updated_at) \
         VALUES (?, ?, 'Fake', 'openai_compatible', ?, 'test-key', 'test-model', 0, 'active', \
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(base_url)
    .execute(state.db.pool())
    .await
    .unwrap();
    id
}

/// Seed an active agent bound to the group. `joined_at` controls fan-out order.
async fn seed_agent(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    provider_id: &str,
    display_name: &str,
    joined_at: &str,
) -> String {
    let agent_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, name, system_prompt, runtime_kind, provider_id, skill_ids_json, \
          status, created_at, updated_at) \
         VALUES (?, ?, ?, 'You are a test agent.', 'llm_chat', ?, '[]', 'active', \
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&agent_id)
    .bind(owner_id)
    .bind(display_name)
    .bind(provider_id)
    .execute(state.db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO group_agents \
         (group_id, agent_id, display_name, status, joined_at, updated_at) \
         VALUES (?, ?, ?, 'active', ?, ?)",
    )
    .bind(group_id)
    .bind(&agent_id)
    .bind(display_name)
    .bind(joined_at)
    .bind(joined_at)
    .execute(state.db.pool())
    .await
    .unwrap();

    agent_id
}

// ---------------------------------------------------------------------------
// Fake LLM provider server (OpenAI-style SSE)
// ---------------------------------------------------------------------------

/// Start a server that answers every request with `body` as an event stream.
async fn fake_provider(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().fallback(move || async move {
        ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn kinds(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn group_stream_uses_monotonic_sequence_not_timestamps() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "seq@example.com").await;
    let owner = owner_id(&state, "seq@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    // free-speech: the single agent responds without an explicit mention.
    let group = create_group(&app, &token, &workspace, json!({"free_speech": true})).await;

    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\
                data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(body).await).await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content": "hi team"}),
    )
    .await;

    // The seq field is a contiguous monotonic counter starting at 0, regardless
    // of (potentially identical) wall-clock timestamps on the persisted rows.
    let seqs: Vec<i64> = events.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    let expected: Vec<i64> = (0..events.len() as i64).collect();
    assert_eq!(seqs, expected, "seq must be contiguous and monotonic");

    // Event ids embed the same sequence, never a timestamp.
    for event in &events {
        let event_id = event["event_id"].as_str().unwrap();
        assert!(event_id.ends_with(&format!(":{}", event["seq"].as_i64().unwrap())));
    }

    let kinds = kinds(&events);
    assert_eq!(kinds.first().unwrap(), "user_message");
    assert_eq!(kinds.last().unwrap(), "done");
    assert!(kinds.contains(&"agent_message".to_string()));
}

#[tokio::test]
async fn group_stream_proactive_silent_turn_does_not_persist_agent_message() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "silent@example.com").await;
    let owner = owner_id(&state, "silent@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;

    // The agent declines its turn with the silent marker.
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"<SILENT>\"}}]}\n\
                data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(body).await).await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content": "anyone around?"}),
    )
    .await;

    let kinds = kinds(&events);
    assert!(kinds.contains(&"agent_silent".to_string()));
    assert!(!kinds.contains(&"agent_message".to_string()));
    // No visible reply → the turn ends in silence.
    assert!(kinds.contains(&"silence".to_string()));

    // The silent turn persisted no agent message row.
    let agent_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE sender_type = 'agent'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(agent_messages, 0);
    // The user message was still persisted.
    let user_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE sender_type = 'user'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(user_messages, 1);
}

#[tokio::test]
async fn group_stream_waiting_for_user_stops_remaining_proactive_fanout() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "waiting@example.com").await;
    let owner = owner_id(&state, "waiting@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;

    // The first agent pauses for human input; the second must never run.
    let body =
        "data: {\"choices\":[{\"delta\":{\"content\":\"<WAITING_FOR_USER> need a budget\"}}]}\n\
                data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(body).await).await;
    let first = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;
    let second = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Bob",
        "2024-01-02T00:00:00Z",
    )
    .await;

    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content": "let's plan"}),
    )
    .await;

    let kinds = kinds(&events);
    assert!(kinds.contains(&"waiting_for_user".to_string()));
    assert_eq!(kinds.last().unwrap(), "done");

    // Only the first agent appears in any agent_start; the second is cut off.
    let started: Vec<&str> = events
        .iter()
        .filter(|e| e["kind"] == "agent_start")
        .map(|e| e["payload"]["agent_id"].as_str().unwrap())
        .collect();
    assert_eq!(started, vec![first.as_str()]);
    assert!(!started.contains(&second.as_str()));

    // Exactly one agent message was persisted (the first agent's).
    let agent_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE sender_type = 'agent'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(agent_messages, 1);
}

#[tokio::test]
async fn group_stream_client_disconnect_cancels_runtime_task() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "cancel@example.com").await;
    let owner = owner_id(&state, "cancel@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"free_speech": true})).await;

    // Stream many tokens so the runtime is still mid-reply when we disconnect.
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"c\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"d\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"e\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"f\"}}]}\n\
                data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(body).await).await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    // Drive the runtime directly through a tiny channel so we can drop the
    // receiver (simulating a client disconnect) mid-stream.
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone());
    let request = TurnRequest {
        group_id: group.clone(),
        owner_id: owner.clone(),
        thread_id: None,
        content: "hi".to_string(),
    };
    let (tx, mut rx) = mpsc::channel(1);
    let handle = tokio::spawn(run_group_turn(services, request, tx));

    // Receive the first two events (user_message, agent_start), then disconnect.
    let first = rx.recv().await.unwrap();
    assert_eq!(first.kind, StreamEventKind::UserMessage);
    let second = rx.recv().await.unwrap();
    assert_eq!(second.kind, StreamEventKind::AgentStart);
    drop(rx);

    let outcome = handle.await.unwrap();
    assert_eq!(outcome, TurnOutcome::Cancelled);

    // Cancellation stopped the turn before any agent message was persisted.
    let agent_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE sender_type = 'agent'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(agent_messages, 0);
}

#[tokio::test]
async fn group_stream_no_routed_agents_ends_in_silence() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "quiet@example.com").await;
    let owner = owner_id(&state, "quiet@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    // Neither free-speech nor proactive, and no @mention → nobody responds.
    let group = create_group(&app, &token, &workspace, json!({})).await;

    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"unused\"}}]}\n\
                data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(body).await).await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content": "just thinking out loud"}),
    )
    .await;

    let kinds = kinds(&events);
    assert!(!kinds.contains(&"agent_start".to_string()));
    assert!(kinds.contains(&"silence".to_string()));
    assert_eq!(kinds.last().unwrap(), "done");
}
