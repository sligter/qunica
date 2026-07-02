//! AgentAsTool group-runtime handoff tests.
//!
//! These tests exercise the runtime directly after seeding groups, agents and
//! providers through the existing API/test database helpers. The fake provider
//! returns a different canned SSE body for each model request so one turn can
//! model both caller and helper responses.

use std::{collections::VecDeque, sync::Arc, time::Duration};

use ag_swarmer_backend::{
    api::{router_with_state_for_tests, AppState},
    runtime::{
        run_group_turn, RuntimeServices, StreamEvent, StreamEventKind, TurnOutcome, TurnRequest,
    },
};
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::IntoResponse,
    Router,
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
async fn agent_as_tool_requires_bound_active_group_member() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-requires@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![tool_body(vec![(
        "call_handoff",
        "AgentAsTool",
        json!({"assistant": "Helper", "task": "take over"}),
    )])])
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
        None,
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Silence);
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
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "user");
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
                json!({"assistant": helper.clone(), "task": "draft summary"}),
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
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, "user");
    assert_eq!(rows[1].1.as_deref(), Some(caller.as_str()));
    assert_eq!(rows[1].2.as_deref(), Some("@Helper draft summary"));
    assert_eq!(rows[2].1.as_deref(), Some(helper.as_str()));
    assert_eq!(rows[2].2.as_deref(), Some("Helper finished"));
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
            json!({"assistant": "Research Lead", "task": "check facts"}),
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
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].2.as_deref(), Some("@Research Lead check facts"));
    assert_eq!(rows[2].1.as_deref(), Some(helper.as_str()));
    assert_eq!(rows[2].2.as_deref(), Some("Facts checked"));
}

#[tokio::test]
async fn agent_as_tool_does_not_dispatch_muted_helper() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-muted@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![tool_body(vec![(
        "call_handoff",
        "AgentAsTool",
        json!({"assistant": "Helper", "task": "take over"}),
    )])])
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
    sqlx::query("UPDATE groups SET muted_agent_ids_json = ? WHERE id = ?")
        .bind(json!([helper.clone()]).to_string())
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller delegate").await;

    assert_eq!(outcome, TurnOutcome::Silence);
    let helper_starts: Vec<&Value> = payloads_of_kind(&events, StreamEventKind::AgentStart)
        .into_iter()
        .filter(|payload| payload["agent_id"] == helper)
        .collect();
    assert!(helper_starts.is_empty(), "muted helper must not run");
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "user");
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
        json!({"assistant": "ACP Helper", "task": "finish externally"}),
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
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].1.as_deref(), Some(helper.as_str()));
    assert_eq!(rows[2].2.as_deref(), Some("ACP helper done"));
}

#[tokio::test]
async fn agent_as_tool_rejects_recursive_self_cycle() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "aat-self@example.com";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace).await;

    let provider_url = fake_provider_sequence(vec![tool_body(vec![(
        "call_self",
        "AgentAsTool",
        json!({"assistant": "Caller", "task": "loop"}),
    )])])
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
    set_tool_config(
        &state,
        &caller,
        json!({"assistant_agents": [{"agent_id": caller, "enabled": true}]}),
    )
    .await;

    let (outcome, events) = run_turn(&state, &group, &owner, "@Caller recurse").await;

    assert_eq!(outcome, TurnOutcome::Silence);
    let results = payloads_of_kind(&events, StreamEventKind::ToolCallResult);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "unavailable");
    assert!(results[0]["result_summary"]
        .as_str()
        .unwrap()
        .contains("cannot delegate to itself"));
    let rows = message_rows(&state).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "user");
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
                json!({"assistant": "helper-agent", "task": "long task"}),
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
