//! AgentAsTool group-runtime handoff tests.
//!
//! These tests exercise the runtime directly after seeding groups, agents and
//! providers through the existing API/test database helpers. The fake provider
//! returns a different canned SSE body for each model request so one turn can
//! model both caller and helper responses.

use std::{collections::VecDeque, sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::IntoResponse,
    Router,
};
use qunica_backend::{
    api::{router_with_state_for_tests, AppState},
    runtime::{
        run_group_turn, RuntimeServices, StreamEvent, StreamEventKind, TurnOutcome, TurnRequest,
    },
};
use serde_json::{json, Value};
use tokio::{sync::mpsc, sync::Mutex, time::timeout};
use tower::ServiceExt;

#[cfg(windows)]
fn write_fake_acp_helper(root: &std::path::Path) -> (String, Vec<String>) {
    let script = root.join("fake-acp-helper.ps1");
    std::fs::write(
        &script,
        r#"
while (($line = [Console]::In.ReadLine()) -ne $null) {
  $request = $line | ConvertFrom-Json
  if ($request.method -eq "initialize") {
    @{ jsonrpc = "2.0"; id = $request.id; result = @{} } | ConvertTo-Json -Compress
  } elseif ($request.method -eq "session/new") {
    @{ jsonrpc = "2.0"; id = $request.id; result = @{ sessionId = "s1" } } | ConvertTo-Json -Compress
  } elseif ($request.method -eq "session/prompt") {
    @{ jsonrpc = "2.0"; method = "session/update"; params = @{ update = @{ sessionUpdate = "agent_message_chunk"; content = @{ type = "text"; text = "ACP helper done" } } } } | ConvertTo-Json -Compress -Depth 8
    @{ jsonrpc = "2.0"; id = $request.id; result = @{ stopReason = "end_turn" } } | ConvertTo-Json -Compress
    break
  } else {
    @{ jsonrpc = "2.0"; id = $request.id; result = @{} } | ConvertTo-Json -Compress
  }
}
"#,
    )
    .unwrap();
    (
        "powershell".to_string(),
        vec![
            "-NoProfile".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            script.to_string_lossy().to_string(),
        ],
    )
}

#[cfg(not(windows))]
fn write_fake_acp_helper(root: &std::path::Path) -> (String, Vec<String>) {
    let script = root.join("fake-acp-helper.sh");
    std::fs::write(
        &script,
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1"}}'
      ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ACP helper done"}}}}'
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
      break
      ;;
    *)
      printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{}}'
      ;;
  esac
done
"#,
    )
    .unwrap();
    ("sh".to_string(), vec![script.to_string_lossy().to_string()])
}

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

async fn create_local_workspace(app: &Router, token: &str) -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let (status, workspace) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            token,
            json!({
                "name": "Local WS",
                "backend_type": "local",
                "local_path": root.path().to_string_lossy()
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    (root, workspace["id"].as_str().unwrap().to_string())
}

async fn create_group(app: &Router, token: &str, workspace_id: &str) -> String {
    let (status, group) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/groups",
            token,
            json!({"name": "Team", "workspace_id": workspace_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    group["id"].as_str().unwrap().to_string()
}

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

#[allow(clippy::too_many_arguments)]
async fn seed_agent(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    provider_id: &str,
    name: &str,
    display_name: &str,
    joined_at: &str,
    tool_config: Option<Value>,
) -> String {
    let agent_id = uuid::Uuid::new_v4().to_string();
    seed_agent_with_id(
        state,
        &agent_id,
        owner_id,
        group_id,
        provider_id,
        name,
        display_name,
        joined_at,
        tool_config,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn seed_agent_with_id(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    group_id: &str,
    provider_id: &str,
    name: &str,
    display_name: &str,
    joined_at: &str,
    tool_config: Option<Value>,
) -> String {
    let tool_config_json = tool_config.map(|value| value.to_string());
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, name, system_prompt, runtime_kind, provider_id, model_config_json, \
          tool_config_json, skill_ids_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'You are a test agent.', 'llm_chat', ?, NULL, ?, '[]', 'active', \
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(agent_id)
    .bind(owner_id)
    .bind(name)
    .bind(provider_id)
    .bind(&tool_config_json)
    .execute(state.db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO group_agents \
         (group_id, agent_id, display_name, status, joined_at, updated_at) \
         VALUES (?, ?, ?, 'active', ?, ?)",
    )
    .bind(group_id)
    .bind(agent_id)
    .bind(display_name)
    .bind(joined_at)
    .bind(joined_at)
    .execute(state.db.pool())
    .await
    .unwrap();

    agent_id.to_string()
}

/// Seed an agent the owner has, but that never joined the group.
///
/// Binding one of these as an assistant is the configuration that used to make
/// delegation vanish: the tool was offered, every call resolved to nothing, and
/// the turn ended without a word.
async fn seed_unjoined_agent(
    state: &AppState,
    owner_id: &str,
    provider_id: &str,
    name: &str,
) -> String {
    let agent_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, name, system_prompt, runtime_kind, provider_id, model_config_json, \
          tool_config_json, skill_ids_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'You are a test agent.', 'llm_chat', ?, NULL, NULL, '[]', 'active', \
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&agent_id)
    .bind(owner_id)
    .bind(name)
    .bind(provider_id)
    .execute(state.db.pool())
    .await
    .unwrap();
    agent_id
}

async fn set_tool_config(state: &AppState, agent_id: &str, tool_config: Value) {
    sqlx::query("UPDATE agents SET tool_config_json = ? WHERE id = ?")
        .bind(tool_config.to_string())
        .bind(agent_id)
        .execute(state.db.pool())
        .await
        .unwrap();
}

async fn fake_provider_sequence(bodies: Vec<String>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let queue = Arc::new(Mutex::new(VecDeque::from(bodies)));
    let app = Router::new().fallback(move || {
        let queue = Arc::clone(&queue);
        async move {
            let body = {
                let mut queue = queue.lock().await;
                queue
                    .pop_front()
                    .unwrap_or_else(|| "data: [DONE]\n".to_string())
            };
            ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Like [`fake_provider_sequence`], but keeps every request body so a test can
/// assert on the tool list the runtime actually advertised.
async fn recording_fake_provider_sequence(bodies: Vec<String>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let queue = Arc::new(Mutex::new(VecDeque::from(bodies)));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback({
        let requests = Arc::clone(&requests);
        move |request: Request<Body>| {
            let queue = Arc::clone(&queue);
            let requests = Arc::clone(&requests);
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap();
                requests
                    .lock()
                    .await
                    .push(serde_json::from_slice(&bytes).unwrap());
                let body = {
                    let mut queue = queue.lock().await;
                    queue
                        .pop_front()
                        .unwrap_or_else(|| "data: [DONE]\n".to_string())
                };
                ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), requests)
}

/// The `AgentAsTool` definition in a recorded provider request, if it was
/// advertised at all.
fn agent_as_tool_schema(request: &Value) -> Option<&Value> {
    request["tools"]
        .as_array()?
        .iter()
        .find(|tool| tool["function"]["name"] == "AgentAsTool")
}

fn text_body(text: &str) -> String {
    format!(
        "data: {}\ndata: [DONE]\n",
        json!({"choices": [{"delta": {"content": text}}]})
    )
}

fn tool_body(calls: Vec<(&str, &str, Value)>) -> String {
    let tool_calls: Vec<Value> = calls
        .into_iter()
        .enumerate()
        .map(|(index, (id, name, args))| {
            json!({
                "index": index,
                "id": id,
                "function": {
                    "name": name,
                    "arguments": args.to_string(),
                },
            })
        })
        .collect();
    format!(
        "data: {}\ndata: [DONE]\n",
        json!({"choices": [{"delta": {"tool_calls": tool_calls}, "finish_reason": "tool_calls"}]})
    )
}

async fn run_turn(
    state: &AppState,
    group_id: &str,
    owner_id: &str,
    content: &str,
) -> (TurnOutcome, Vec<StreamEvent<Value>>) {
    let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone());
    let request = TurnRequest {
        group_id: group_id.to_string(),
        owner_id: owner_id.to_string(),
        thread_id: None,
        content: content.to_string(),
        attachments: Vec::new(),
        model_override: None,
        effort_override: None,
    };
    let (tx, mut rx) = mpsc::channel(64);
    let handle = tokio::spawn(run_group_turn(services, request, tx));
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    (handle.await.unwrap(), events)
}

fn payloads_of_kind(events: &[StreamEvent<Value>], kind: StreamEventKind) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event.kind == kind)
        .map(|event| &event.payload)
        .collect()
}

async fn message_rows(state: &AppState) -> Vec<(String, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT sender_type, sender_id, content FROM messages \
         WHERE status = 'visible' ORDER BY seq ASC",
    )
    .fetch_all(state.db.pool())
    .await
    .unwrap()
}

#[tokio::test]
#[allow(clippy::type_complexity)]
async fn agent_as_tool_call_is_private_and_returns_to_caller() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-bounded-call@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;
    let helper_id = uuid::Uuid::new_v4().to_string();
    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_private",
            "AgentAsTool",
            json!({"assistant": helper_id, "task": "inspect privately", "mode": "call"}),
        )]),
        text_body("private findings"),
        text_body("caller final answer"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent_with_id(
        &state,
        &helper_id,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller investigate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("caller final answer"));
    assert!(payloads_of_kind(&events, StreamEventKind::AgentMessage)
        .iter()
        .all(|payload| payload["agent_id"] != helper));

    let dispatches: Vec<(
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT parent_dispatch_id, source_agent_id, action_kind, status, artifact_json, \
                    output_message_id FROM agent_dispatches ORDER BY hop, created_at",
    )
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(dispatches.len(), 2);
    assert_eq!(dispatches[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(dispatches[1].2, "call");
    assert_eq!(dispatches[1].3, "completed");
    assert!(dispatches[1]
        .4
        .as_deref()
        .unwrap()
        .contains("private findings"));
    assert_eq!(dispatches[1].5, None);
}

#[tokio::test]
async fn agent_as_tool_runs_each_helper_at_most_once_per_turn() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-once-per-turn@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;
    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "first_call",
            "AgentAsTool",
            json!({"assistant": "Helper", "task": "inspect", "mode": "call"}),
        )]),
        text_body("first findings"),
        tool_body(vec![(
            "duplicate_call",
            "AgentAsTool",
            json!({"assistant": "Helper", "task": "inspect again", "mode": "call"}),
        )]),
        text_body("caller final answer"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller investigate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["status"], "completed");
    assert_eq!(results[1]["status"], "already_scheduled");
    let dispatch_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_dispatches")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(dispatch_count, 2, "one public dispatch and one helper call");
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("caller final answer"));
}

/// One `fan_out` reaches every assistant it names and answers once. The point
/// is the round trip: three helpers used to cost the caller three provider
/// requests, each carrying its whole context, and now cost one.
#[tokio::test]
#[allow(clippy::type_complexity)]
async fn agent_as_tool_fan_out_runs_every_assistant_and_returns_one_result() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-fan-out@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;
    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_fan_out",
            "AgentAsTool",
            json!({
                "mode": "fan_out",
                "dispatches": [
                    {"assistant": "Alice", "task": "review module a"},
                    {"assistant": "Bob", "task": "review module b"},
                ],
            }),
        )]),
        text_body("alice findings"),
        text_body("bob findings"),
        text_body("caller final answer"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let alice = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "alice-agent",
        "Alice",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let bob = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "bob-agent",
        "Bob",
        "2024-01-03T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [
            {"agent_id": alice, "enabled": true},
            {"agent_id": bob, "enabled": true},
        ]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller investigate").await;

    assert_eq!(outcome, TurnOutcome::Completed);

    // One call in, one result out, holding both helpers' work.
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "completed");
    let summary = results[0]["result_summary"].as_str().unwrap();
    assert!(summary.contains("2 assistants, 2 completed"), "{summary}");

    // Both helpers ran privately: the transcript holds the user's message and
    // the caller's answer, and nothing either helper said.
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("caller final answer"));

    // Fan-out targets are private calls with siblings, so the trace shows two
    // `call` dispatches under the caller rather than a shape nothing else reads.
    let dispatches: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT action_kind, status, target_agent_id, artifact_json FROM agent_dispatches \
         WHERE action_kind = 'call' ORDER BY created_at",
    )
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(dispatches.len(), 2);
    assert!(dispatches.iter().all(|row| row.1 == "completed"));
    assert_eq!(dispatches[0].2.as_deref(), Some(alice.as_str()));
    assert_eq!(dispatches[1].2.as_deref(), Some(bob.as_str()));
    assert!(dispatches[0]
        .3
        .as_deref()
        .unwrap()
        .contains("alice findings"));
    assert!(dispatches[1].3.as_deref().unwrap().contains("bob findings"));
}

/// A target that cannot run is one labelled section, not the end of the call.
/// Aborting the batch would strand the helpers that already ran: each is spent
/// for the turn, so there would be nothing left for the caller to retry.
#[tokio::test]
async fn agent_as_tool_fan_out_reports_a_dead_target_and_keeps_the_rest() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-fan-out-partial@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;
    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_fan_out",
            "AgentAsTool",
            json!({
                "mode": "fan_out",
                "dispatches": [
                    {"assistant": "Ghost", "task": "review module a"},
                    {"assistant": "Bob", "task": "review module b"},
                ],
            }),
        )]),
        text_body("bob findings"),
        text_body("caller final answer"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let alice = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "alice-agent",
        "Alice",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let bob = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "bob-agent",
        "Bob",
        "2024-01-03T00:00:00Z",
        None,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [
            {"agent_id": alice, "enabled": true},
            {"agent_id": bob, "enabled": true},
        ]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller investigate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    // Something ran, so the call completed; the aggregate carries the reason
    // the other target did not, under the name the model used for it.
    assert_eq!(results[0]["status"], "completed");
    let summary = results[0]["result_summary"].as_str().unwrap();
    assert!(summary.contains("2 assistants, 1 completed"), "{summary}");
    assert!(summary.contains("@Ghost"), "{summary}");

    let dispatch_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_dispatches WHERE action_kind = 'call'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(
        dispatch_count, 1,
        "only the resolvable target was dispatched"
    );

    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].2.as_deref(), Some("caller final answer"));
}

#[tokio::test]
#[allow(clippy::type_complexity)]
async fn agent_as_tool_omitted_mode_is_rejected_and_caller_can_recover() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-bounded-handoff@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;
    let helper_id = uuid::Uuid::new_v4().to_string();
    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_handoff",
            "AgentAsTool",
            json!({"assistant": helper_id, "task": "take over"}),
        )]),
        text_body("helper visible answer"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent_with_id(
        &state,
        &helper_id,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "failed");
    assert!(results[0]["result_summary"]
        .as_str()
        .unwrap()
        .contains("mode is required"));
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("helper visible answer"));

    let dispatches: Vec<(
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, parent_dispatch_id, source_agent_id, action_kind, status, output_message_id \
             FROM agent_dispatches ORDER BY hop, created_at",
    )
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].3, "speak");
    assert_eq!(dispatches[0].4, "completed");
    assert!(dispatches[0].5.is_some());
}

#[tokio::test]
async fn agent_as_tool_requires_bound_active_group_member() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-requires@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_handoff",
            "AgentAsTool",
            json!({"assistant": "Helper", "task": "take over", "mode": "handoff"}),
        )]),
        text_body("Helper is not bound to me, so I answered myself."),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        None,
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["tool_name"], "AgentAsTool");
    assert_eq!(results[0]["status"], "unavailable");
    assert!(results[0]["result_summary"]
        .as_str()
        .unwrap()
        .contains("not enabled"));
    let helper_starts: Vec<&Value> = payloads_of_kind(&events, StreamEventKind::AgentStart)
        .into_iter()
        .filter(|payload| payload["agent_id"] == helper)
        .collect();
    assert!(helper_starts.is_empty(), "unbound helper must not run");

    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "user");
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
}

#[tokio::test]
async fn agent_as_tool_terminal_handoff_skips_sibling_tools() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-terminal@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let helper = uuid::Uuid::new_v4().to_string();
    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![
            (
                "call_sibling",
                "Bash",
                json!({"command": "echo should-not-run"}),
            ),
            (
                "call_handoff",
                "AgentAsTool",
                json!({"assistant": helper.clone(), "task": "draft summary", "mode": "handoff"}),
            ),
        ]),
        text_body("Helper finished"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent_with_id(
        &state,
        &helper,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let starts = payloads_of_kind(&events, StreamEventKind::ToolCallStart);
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0]["tool_name"], "AgentAsTool");
    assert_eq!(starts[0]["tool_call_id"], "call_handoff");
    assert!(
        starts
            .iter()
            .all(|payload| payload["tool_name"].as_str().unwrap() != "Bash"),
        "sibling tools must not execute in a terminal handoff"
    );

    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "user");
    assert_eq!(rows[1].1.as_deref(), Some(helper.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("Helper finished"));
    assert!(rows
        .iter()
        .all(|row| row.1.as_deref() != Some(caller.as_str())));
}

#[tokio::test]
async fn agent_as_tool_resolves_group_display_name() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-display@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_handoff",
            "AgentAsTool",
            json!({"assistant": "Research Lead", "task": "check facts", "mode": "handoff"}),
        )]),
        text_body("Facts checked"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Research Lead",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, _events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(helper.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("Facts checked"));
}

#[tokio::test]
async fn agent_as_tool_does_not_dispatch_muted_helper() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-muted@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_handoff",
            "AgentAsTool",
            json!({"assistant": "Helper", "task": "take over", "mode": "handoff"}),
        )]),
        text_body("Helper is muted, so I answered myself."),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;
    sqlx::query("UPDATE groups SET muted_agent_ids_json = ? WHERE id = ?")
        .bind(json!([helper.clone()]).to_string())
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let helper_starts: Vec<&Value> = payloads_of_kind(&events, StreamEventKind::AgentStart)
        .into_iter()
        .filter(|payload| payload["agent_id"] == helper)
        .collect();
    assert!(helper_starts.is_empty(), "muted helper must not run");
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "user");
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
}

#[tokio::test]
async fn agent_as_tool_runs_acp_helper_with_full_group_context() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-acp-helper@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let (root, workspace) = create_local_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let (acp_command, acp_args) = write_fake_acp_helper(root.path());

    let provider_url = fake_provider_sequence(vec![tool_body(vec![(
        "call_handoff",
        "AgentAsTool",
        json!({"assistant": "ACP Helper", "task": "finish externally", "mode": "handoff"}),
    )])])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, workspace_id, name, system_prompt, runtime_kind, provider_id, \
          external_runtime_json, skill_ids_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'acp-helper', 'You are an ACP helper.', 'acp', NULL, ?, '[]', \
                 'active', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&helper)
    .bind(&owner)
    .bind(&workspace)
    .bind(
        json!({
            "command": acp_command,
            "args": acp_args,
            "timeout_seconds": 10
        })
        .to_string(),
    )
    .execute(state.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO group_agents \
         (group_id, agent_id, display_name, context_scope_json, status, joined_at, updated_at) \
         VALUES (?, ?, 'ACP Helper', '{\"share_group_workspace\":true}', 'active', \
                 '2024-01-02T00:00:00Z', '2024-01-02T00:00:00Z')",
    )
    .bind(&group)
    .bind(&helper)
    .execute(state.db.pool())
    .await
    .unwrap();
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    assert!(payloads_of_kind(&events, StreamEventKind::AcpAgentRun)
        .iter()
        .any(|payload| payload["status"] == "completed"));
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(helper.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("ACP helper done"));
}

#[tokio::test]
async fn agent_as_tool_claims_a_pending_public_responder_without_running_it_twice() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-self@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_public",
            "AgentAsTool",
            json!({"assistant": "Public Helper", "task": "duplicate", "mode": "call"}),
        )]),
        text_body("I kept the public turn."),
        text_body("I ran once through the scheduler."),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        None,
    )
    .await;
    let public_helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "public-helper",
        "Public Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let spare_helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "spare-helper",
        "Spare Helper",
        "2024-01-03T00:00:00Z",
        None,
    )
    .await;
    set_tool_config(
        &state,
        &caller,
        json!({"assistant_agents": [
            {"agent_id": public_helper, "enabled": true},
            {"agent_id": spare_helper, "enabled": true}
        ]}),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller then @Public Helper").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "completed");
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(
        rows[1].2.as_deref(),
        Some("I ran once through the scheduler.")
    );
    let dispatch_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_dispatches")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(dispatch_count, 2, "one public dispatch and one helper call");
}

/// The tool has to name its targets. Without this the `assistant` field was an
/// unconstrained string and the description named nobody, so the model had to
/// guess an identifier and a wrong guess looked exactly like the feature not
/// existing.
#[tokio::test]
async fn agent_as_tool_schema_names_the_assistants_the_caller_can_reach() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-schema@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let (provider_url, requests) =
        recording_fake_provider_sequence(vec![text_body("nothing to delegate")]).await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Research Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, _events) = run_turn(&state, &group, &owner, "@Caller answer").await;
    assert_eq!(outcome, TurnOutcome::Completed);

    let requests = requests.lock().await;
    let schema = agent_as_tool_schema(&requests[0]).expect("AgentAsTool must be advertised");
    let description = schema["function"]["description"].as_str().unwrap();
    assert!(
        description.contains("@Research Helper (helper-agent)"),
        "description must name the reachable assistant: {description}"
    );
    let parameters = &schema["function"]["parameters"];
    assert_eq!(
        parameters["properties"]["assistant"]["enum"],
        json!(["Research Helper"]),
        "the selector must be constrained to names that resolve"
    );
    assert_eq!(
        parameters["properties"]["mode"]["enum"],
        json!(["call", "handoff"]),
        "mode must enumerate its two values rather than take any string"
    );
}

/// An assistant bound but absent from the group makes the tool unusable, so it
/// is withheld and the misconfiguration is said out loud instead of leaving the
/// owner to infer it from an agent that never delegates.
#[tokio::test]
async fn agent_as_tool_is_withheld_and_warned_when_no_assistant_is_in_the_group() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-withheld@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let (provider_url, requests) =
        recording_fake_provider_sequence(vec![text_body("answering directly")]).await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let outsider = seed_unjoined_agent(&state, &owner, &provider, "outsider-agent").await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": outsider, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller answer").await;
    assert_eq!(outcome, TurnOutcome::Completed);

    let requests = requests.lock().await;
    assert!(
        agent_as_tool_schema(&requests[0]).is_none(),
        "a tool that cannot name a single reachable assistant must not be advertised"
    );
    let warnings = payloads_of_kind(&events, StreamEventKind::Warning);
    assert!(
        warnings.iter().any(|payload| payload["message"]
            .as_str()
            .is_some_and(|message| message.contains("AgentAsTool is unavailable"))),
        "the owner must be told why delegation is not available: {warnings:?}"
    );
}

/// The whole point of the fix: a rejected dispatch comes back as a tool result,
/// so the caller can name a different assistant and finish its turn. This used
/// to end the turn outright — no reply, no result, no reason.
#[tokio::test]
async fn agent_as_tool_unavailable_assistant_lets_the_caller_retry_and_answer() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-retry@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_wrong",
            "AgentAsTool",
            json!({"assistant": "Ghost", "task": "research", "mode": "call"}),
        )]),
        tool_body(vec![(
            "call_right",
            "AgentAsTool",
            json!({"assistant": "@Helper", "task": "research", "mode": "call"}),
        )]),
        text_body("helper findings"),
        text_body("caller final answer"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let ghost = seed_unjoined_agent(&state, &owner, &provider, "Ghost").await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [
            {"agent_id": ghost, "enabled": true},
            {"agent_id": helper, "enabled": true},
        ]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller investigate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 2, "both the rejection and the retry report");
    assert_eq!(results[0]["status"], "unavailable");
    assert!(results[0]["result_summary"]
        .as_str()
        .unwrap()
        .contains("must be added to this group"));
    assert_eq!(results[1]["status"], "completed");

    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2, "the helper answered privately");
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("caller final answer"));
}

/// A rejected handoff returns control to the caller instead of swallowing the
/// turn.
#[tokio::test]
async fn agent_as_tool_handoff_rejection_returns_a_tool_result_instead_of_ending_the_turn() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-handoff-recover@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_handoff",
            "AgentAsTool",
            json!({"assistant": "Ghost", "task": "take over", "mode": "handoff"}),
        )]),
        text_body("Ghost is not in this group, so I answered myself."),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let ghost = seed_unjoined_agent(&state, &owner, &provider, "Ghost").await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [
            {"agent_id": ghost, "enabled": true},
            {"agent_id": helper, "enabled": true},
        ]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "unavailable");

    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2, "the caller must still get to answer");
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(
        rows[1].2.as_deref(),
        Some("Ghost is not in this group, so I answered myself.")
    );
}

/// A `mode` the enum does not contain is a malformed call, not a reason to lose
/// the turn: the model is told what the valid values are and gets to try again.
#[tokio::test]
async fn agent_as_tool_unparseable_mode_is_reported_and_recoverable() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-bad-mode@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_bad_mode",
            "AgentAsTool",
            json!({"assistant": "Helper", "task": "research", "mode": "delegate"}),
        )]),
        text_body("Retrying without the bad mode."),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "helper-agent",
        "Helper",
        "2024-01-02T00:00:00Z",
        None,
    )
    .await;
    let caller = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "caller-agent",
        "Caller",
        "2024-01-01T00:00:00Z",
        Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller investigate").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "failed");
    assert!(results[0]["result_summary"]
        .as_str()
        .unwrap()
        .contains("call, handoff, or fan_out"));

    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
}

mod agent_as_tool {
    use super::*;

    #[tokio::test]
    async fn dropped_subscriber_does_not_cancel_helper_dispatch() {
        let (app, state) = router_with_state_for_tests().await;
        let email = "aat-cancel@example.com";
        let token = register_and_login(&app, email).await;
        let owner = owner_id(&state, email).await;
        let workspace = create_workspace(&app, &token).await;
        let group = create_group(&app, &token, &workspace).await;

        let provider_url = fake_provider_sequence(vec![
            tool_body(vec![(
                "call_handoff",
                "AgentAsTool",
                json!({"assistant": "helper-agent", "task": "long task", "mode": "handoff"}),
            )]),
            (0..16)
                .map(|index| {
                    format!(
                        "data: {}\n",
                        json!({"choices": [{"delta": {"content": index.to_string()}}]})
                    )
                })
                .collect::<String>()
                + "data: [DONE]\n",
        ])
        .await;
        let provider = seed_provider(&state, &owner, &provider_url).await;
        let helper = seed_agent(
            &state,
            &owner,
            &group,
            &provider,
            "helper-agent",
            "Helper",
            "2024-01-02T00:00:00Z",
            None,
        )
        .await;
        seed_agent(
            &state,
            &owner,
            &group,
            &provider,
            "caller-agent",
            "Caller",
            "2024-01-01T00:00:00Z",
            Some(json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]})),
        )
        .await;

        let services = RuntimeServices::new(state.db.pool().clone(), state.write_lock.clone());
        let request = TurnRequest {
            group_id: group.clone(),
            owner_id: owner.clone(),
            thread_id: None,
            content: "@Caller delegate".to_string(),
            attachments: Vec::new(),
            model_override: None,
            effort_override: None,
        };
        let (tx, mut rx) = mpsc::channel(1);
        let handle = tokio::spawn(run_group_turn(services, request, tx));

        let saw_helper_token = timeout(Duration::from_secs(5), async {
            let mut helper_started = false;
            while let Some(event) = rx.recv().await {
                if event.kind == StreamEventKind::AgentStart && event.payload["agent_id"] == helper
                {
                    helper_started = true;
                    continue;
                }
                if helper_started && event.kind == StreamEventKind::Token {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap();
        assert!(
            saw_helper_token,
            "helper should stream content before cancellation"
        );
        drop(rx);

        let outcome = timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome, TurnOutcome::Completed);

        let helper_messages: Vec<(Option<String>, String)> =
            sqlx::query_as("SELECT content, status FROM messages WHERE sender_id = ?")
                .bind(&helper)
                .fetch_all(state.db.pool())
                .await
                .unwrap();
        assert_eq!(helper_messages.len(), 1);
        assert_eq!(
            helper_messages[0].0.as_deref(),
            Some("0123456789101112131415")
        );
        assert_eq!(helper_messages[0].1, "visible");
    }
}
