//! Group streaming runtime integration tests.
//!
//! Each test seeds users/groups via the public API and seeds agents, group
//! bindings and LLM providers directly through the shared pool (there is no
//! group-agent or provider binding API yet). LLM streaming is exercised against
//! a local fake HTTP server that replays canned provider-specific SSE; no live
//! external API is contacted.

use std::{collections::VecDeque, sync::Arc};

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
use tokio::sync::{mpsc, Mutex};
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

fn authed_empty(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
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
    seed_provider_kind(state, owner_id, "openai-compatible", base_url).await
}

async fn seed_provider_kind(
    state: &AppState,
    owner_id: &str,
    kind: &str,
    base_url: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO llm_providers \
         (id, owner_id, name, kind, base_url, api_key, default_model, reasoning_passback, \
          status, created_at, updated_at) \
         VALUES (?, ?, 'Fake', ?, ?, 'test-key', 'test-model', 0, 'active', \
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(kind)
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

async fn seed_agent_with_tool_config(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    provider_id: &str,
    display_name: &str,
    joined_at: &str,
    tool_config: Value,
) -> String {
    let agent_id = seed_agent(
        state,
        owner_id,
        group_id,
        provider_id,
        display_name,
        joined_at,
    )
    .await;
    sqlx::query("UPDATE agents SET tool_config_json = ? WHERE id = ?")
        .bind(tool_config.to_string())
        .bind(&agent_id)
        .execute(state.db.pool())
        .await
        .unwrap();
    agent_id
}

async fn seed_thread(state: &AppState, group_id: &str, status: &str) -> String {
    let thread_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO threads (id, group_id, agent_id, status, next_seq, created_at, updated_at) \
         VALUES (?, ?, NULL, ?, 1, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&thread_id)
    .bind(group_id)
    .bind(status)
    .execute(state.db.pool())
    .await
    .unwrap();
    thread_id
}

async fn seed_message(
    state: &AppState,
    group_id: &str,
    thread_id: &str,
    seq: i64,
    status: &str,
    sender_type: &str,
    sender_id: Option<&str>,
    content: &str,
    content_json: Option<Value>,
) -> String {
    let message_id = uuid::Uuid::new_v4().to_string();
    let content_json = content_json.map(|value| value.to_string());
    sqlx::query(
        "INSERT INTO messages \
         (id, thread_id, group_id, seq, sender_type, sender_id, message_type, content, \
          content_json, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'text', ?, ?, ?, '2024-01-01T00:00:00Z')",
    )
    .bind(&message_id)
    .bind(thread_id)
    .bind(group_id)
    .bind(seq)
    .bind(sender_type)
    .bind(sender_id)
    .bind(content)
    .bind(content_json)
    .bind(status)
    .execute(state.db.pool())
    .await
    .unwrap();

    sqlx::query("UPDATE threads SET next_seq = MAX(next_seq, ?) WHERE id = ?")
        .bind(seq + 1)
        .bind(thread_id)
        .execute(state.db.pool())
        .await
        .unwrap();

    message_id
}

async fn seed_stream_event(state: &AppState, thread_id: &str) -> String {
    let event_id = format!("test-event:{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO stream_events \
         (id, stream_id, thread_id, seq, event_id, kind, payload_json, created_at) \
         VALUES (?, ?, ?, 0, ?, 'done', '{}', '2024-01-01T00:00:00Z')",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(thread_id)
    .bind(&event_id)
    .execute(state.db.pool())
    .await
    .unwrap();
    event_id
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
async fn messages_list_returns_chronological_visible_interrupted_shape() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "messages-shape@example.com").await;
    let owner = owner_id(&state, "messages-shape@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let thread = seed_thread(&state, &group, "active").await;
    let agent_id = uuid::Uuid::new_v4().to_string();

    seed_message(
        &state,
        &group,
        &thread,
        1,
        "visible",
        "user",
        Some(&owner),
        "one",
        None,
    )
    .await;
    seed_message(
        &state,
        &group,
        &thread,
        2,
        "cleared",
        "agent",
        Some(&agent_id),
        "two-cleared",
        None,
    )
    .await;
    seed_message(
        &state,
        &group,
        &thread,
        3,
        "interrupted",
        "agent",
        Some(&agent_id),
        "three",
        Some(json!({
            "context_usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "context_window_tokens": 100,
                "output_reserve_tokens": 20,
                "ratio": 0.15,
                "source": "prompt",
                "updated_at": "2024-01-01T00:00:00Z"
            }
        })),
    )
    .await;
    seed_message(
        &state,
        &group,
        &thread,
        4,
        "visible",
        "agent",
        Some(&agent_id),
        "four",
        Some(json!({"context_usage": "not-an-object"})),
    )
    .await;

    let (status, body) = send(
        &app,
        authed_empty(
            "GET",
            &format!("/api/v2/groups/{group}/messages?limit=30"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let messages = body.as_array().unwrap();
    let contents: Vec<&str> = messages
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect();
    assert_eq!(contents, vec!["one", "three", "four"]);

    assert_eq!(messages[0]["group_id"].as_str().unwrap(), group);
    assert_eq!(messages[0]["thread_id"].as_str().unwrap(), thread);
    assert_eq!(messages[0]["sender_type"], "user");
    assert_eq!(messages[0]["sender_id"].as_str().unwrap(), owner);
    assert_eq!(messages[0]["message_type"], "text");
    assert_eq!(messages[0]["status"], "visible");
    assert!(messages[0]["refs"].is_null());
    assert!(messages[0]["reply_to_message_id"].is_null());
    assert!(messages[0]["context_usage"].is_null());
    assert!(messages[0]["created_at"].as_str().is_some());

    assert_eq!(messages[1]["status"], "interrupted");
    assert_eq!(messages[1]["context_usage"]["input_tokens"], 10);
    assert_eq!(messages[1]["context_usage"]["source"], "prompt");
    assert!(messages[2]["context_usage"].is_null());
}

#[tokio::test]
async fn messages_list_clamps_limit_and_paginates_before_oldest_id() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "messages-page@example.com").await;
    let owner = owner_id(&state, "messages-page@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let thread = seed_thread(&state, &group, "active").await;
    let mut ids = Vec::new();

    for seq in 1..=105 {
        let id = seed_message(
            &state,
            &group,
            &thread,
            seq,
            "visible",
            "user",
            Some(&owner),
            &format!("msg-{seq}"),
            None,
        )
        .await;
        ids.push(id);
    }

    let (status, body) = send(
        &app,
        authed_empty(
            "GET",
            &format!("/api/v2/groups/{group}/messages?limit=999"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page = body.as_array().unwrap();
    assert_eq!(page.len(), 100);
    assert_eq!(page[0]["id"].as_str().unwrap(), ids[5]);
    assert_eq!(page[0]["content"], "msg-6");
    assert_eq!(page[99]["content"], "msg-105");

    let before = page[0]["id"].as_str().unwrap();
    let (status, body) = send(
        &app,
        authed_empty(
            "GET",
            &format!("/api/v2/groups/{group}/messages?limit=999&before={before}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let older_page = body.as_array().unwrap();
    assert_eq!(older_page.len(), 5);
    assert_eq!(older_page[0]["content"], "msg-1");
    assert_eq!(older_page[4]["content"], "msg-5");

    let (status, body) = send(
        &app,
        authed_empty(
            "GET",
            &format!("/api/v2/groups/{group}/messages?limit=0"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let lower_clamped = body.as_array().unwrap();
    assert_eq!(lower_clamped.len(), 1);
    assert_eq!(lower_clamped[0]["content"], "msg-105");
}

#[tokio::test]
async fn messages_list_rejects_invalid_before_targets() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "messages-before@example.com").await;
    let owner = owner_id(&state, "messages-before@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let other_group = create_group(&app, &token, &workspace, json!({"name": "Other"})).await;
    let thread = seed_thread(&state, &group, "active").await;
    let other_thread = seed_thread(&state, &other_group, "active").await;

    let cleared = seed_message(
        &state,
        &group,
        &thread,
        1,
        "cleared",
        "user",
        Some(&owner),
        "cleared",
        None,
    )
    .await;
    let outside = seed_message(
        &state,
        &other_group,
        &other_thread,
        1,
        "visible",
        "user",
        Some(&owner),
        "outside",
        None,
    )
    .await;

    for before in [outside, cleared] {
        let (status, body) = send(
            &app,
            authed_empty(
                "GET",
                &format!("/api/v2/groups/{group}/messages?before={before}"),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    let missing_group = uuid::Uuid::new_v4();
    let (status, body) = send(
        &app,
        authed_empty(
            "GET",
            &format!("/api/v2/groups/{missing_group}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn messages_list_and_clear_reject_cross_owner_access() {
    let (app, _state) = router_with_state_for_tests().await;
    let owner_token = register_and_login(&app, "messages-owner@example.com").await;
    let intruder_token = register_and_login(&app, "messages-intruder@example.com").await;
    let workspace = create_workspace(&app, &owner_token).await;
    let group = create_group(&app, &owner_token, &workspace, json!({})).await;

    let (status, body) = send(
        &app,
        authed_empty(
            "GET",
            &format!("/api/v2/groups/{group}/messages"),
            &intruder_token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, body) = send(
        &app,
        authed_empty(
            "POST",
            &format!("/api/v2/groups/{group}/messages/clear"),
            &intruder_token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn message_send_rejects_empty_trimmed_content() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-empty@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "  \n\t  "}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn message_send_rejects_cross_owner_group_access() {
    let (app, _state) = router_with_state_for_tests().await;
    let owner_token = register_and_login(&app, "send-owner@example.com").await;
    let intruder_token = register_and_login(&app, "send-intruder@example.com").await;
    let workspace = create_workspace(&app, &owner_token).await;
    let group = create_group(&app, &owner_token, &workspace, json!({})).await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &intruder_token,
            json!({"content": "hello"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn message_send_no_routed_agents_returns_user_message_without_provider() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-quiet@example.com").await;
    let owner = owner_id(&state, "send-quiet@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let missing_provider = uuid::Uuid::new_v4().to_string();
    seed_agent(
        &state,
        &owner,
        &group,
        &missing_provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "  just thinking out loud  "}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["user_message"]["content"], "just thinking out loud");
    assert_eq!(body["user_message"]["sender_type"], "user");
    assert_eq!(body["user_message"]["sender_id"].as_str().unwrap(), owner);
    assert!(body["agent_replies"].as_array().unwrap().is_empty());
    assert!(body["dispatch_messages"].as_array().unwrap().is_empty());
    assert!(body["warnings"].as_array().unwrap().is_empty());
    assert!(body["silent_turns"].as_array().unwrap().is_empty());
    assert_eq!(body["all_silent"], false);
    assert_eq!(body["waiting_for_user"], false);

    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(messages, 1);
}

#[tokio::test]
async fn message_send_free_speech_one_agent_returns_persisted_reply_and_history() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-happy@example.com").await;
    let owner = owner_id(&state, "send-happy@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"free_speech": true})).await;

    let provider_body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
                         data: {\"choices\":[{\"delta\":{\"content\":\" from fake\"}}]}\n\
                         data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(provider_body).await).await;
    let agent = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "hi team"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["user_message"]["content"], "hi team");
    let replies = body["agent_replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["content"], "Hello from fake");
    assert_eq!(replies[0]["sender_id"].as_str().unwrap(), agent);
    assert!(body["dispatch_messages"].as_array().unwrap().is_empty());
    assert!(body["warnings"].as_array().unwrap().is_empty());
    assert_eq!(body["all_silent"], false);
    assert_eq!(body["waiting_for_user"], false);

    let (status, history) = send(
        &app,
        authed_empty("GET", &format!("/api/v2/groups/{group}/messages"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let contents: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect();
    assert_eq!(contents, vec!["hi team", "Hello from fake"]);
}

#[tokio::test]
async fn message_send_proactive_silent_turn_returns_warning_without_agent_row() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-silent@example.com").await;
    let owner = owner_id(&state, "send-silent@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;

    let provider_body = "data: {\"choices\":[{\"delta\":{\"content\":\"<SILENT>\"}}]}\n\
                         data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(provider_body).await).await;
    let agent = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Alice",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "anyone around?"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body["agent_replies"].as_array().unwrap().is_empty());
    assert!(body["dispatch_messages"].as_array().unwrap().is_empty());
    assert_eq!(body["silent_turns"].as_array().unwrap().len(), 1);
    assert_eq!(body["silent_turns"][0]["agent_id"].as_str().unwrap(), agent);
    assert_eq!(body["silent_turns"][0]["display_name"], "Alice");
    assert_eq!(body["all_silent"], true);
    assert_eq!(body["waiting_for_user"], false);
    assert_eq!(body["warnings"], json!(["No one replied"]));

    let agent_messages: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE group_id = ? AND sender_type = 'agent'",
    )
    .bind(&group)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(agent_messages, 0);
}

#[tokio::test]
async fn message_send_waiting_for_user_returns_reply_and_stops_fanout() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-waiting@example.com").await;
    let owner = owner_id(&state, "send-waiting@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;

    let provider_body =
        "data: {\"choices\":[{\"delta\":{\"content\":\"<WAITING_FOR_USER> need a budget\"}}]}\n\
         data: [DONE]\n";
    let provider = seed_provider(&state, &owner, &fake_provider(provider_body).await).await;
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

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "let's plan"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let replies = body["agent_replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["sender_id"].as_str().unwrap(), first);
    assert_eq!(replies[0]["content"], "need a budget");
    assert_eq!(body["waiting_for_user"], true);
    assert_eq!(body["all_silent"], false);
    assert!(body["silent_turns"].as_array().unwrap().is_empty());

    let second_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ? AND sender_id = ?")
            .bind(&group)
            .bind(&second)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(second_messages, 0);
}

#[tokio::test]
async fn message_send_agent_as_tool_splits_dispatch_and_helper_reply() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-aat@example.com").await;
    let owner = owner_id(&state, "send-aat@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;

    let provider_url = fake_provider_sequence(vec![
        tool_body(vec![(
            "call_handoff",
            "AgentAsTool",
            json!({"assistant": "Helper", "task": "draft summary"}),
        )]),
        text_body("Helper finished"),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &provider_url).await;
    let helper = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Helper",
        "2024-01-02T00:00:00Z",
    )
    .await;
    let caller = seed_agent_with_tool_config(
        &state,
        &owner,
        &group,
        &provider,
        "Caller",
        "2024-01-01T00:00:00Z",
        json!({"assistant_agents": [{"agent_id": helper, "enabled": true}]}),
    )
    .await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "@Caller delegate"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let dispatches = body["dispatch_messages"].as_array().unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0]["sender_id"].as_str().unwrap(), caller);
    assert_eq!(dispatches[0]["content"], "@Helper draft summary");
    let replies = body["agent_replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["sender_id"].as_str().unwrap(), helper);
    assert_eq!(replies[0]["content"], "Helper finished");
    assert!(body["warnings"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn message_send_bad_thread_id_errors_without_user_message() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "send-bad-thread@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let stale_thread = seed_thread(&state, &group, "cleared").await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group}/messages"),
            &token,
            json!({"content": "hello", "thread_id": stale_thread}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("active thread"));
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(messages, 0);
}

#[tokio::test]
async fn messages_clear_marks_visible_history_and_preserves_rows() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "messages-clear@example.com").await;
    let owner = owner_id(&state, "messages-clear@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let thread = seed_thread(&state, &group, "active").await;
    let event_id = seed_stream_event(&state, &thread).await;

    seed_message(
        &state,
        &group,
        &thread,
        1,
        "visible",
        "user",
        Some(&owner),
        "visible",
        None,
    )
    .await;
    seed_message(
        &state,
        &group,
        &thread,
        2,
        "interrupted",
        "user",
        Some(&owner),
        "interrupted",
        None,
    )
    .await;
    seed_message(
        &state,
        &group,
        &thread,
        3,
        "cleared",
        "user",
        Some(&owner),
        "already-cleared",
        None,
    )
    .await;

    let (status, body) = send(
        &app,
        authed_empty(
            "POST",
            &format!("/api/v2/groups/{group}/messages/clear"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cleared_count"].as_u64().unwrap(), 2);

    let (status, body) = send(
        &app,
        authed_empty("GET", &format!("/api/v2/groups/{group}/messages"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().unwrap().is_empty());

    let total_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ?")
            .bind(&group)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(total_messages, 3);
    let visible_messages: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages \
         WHERE group_id = ? AND status IN ('visible', 'interrupted')",
    )
    .bind(&group)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(visible_messages, 0);
    let stream_event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM stream_events WHERE event_id = ?")
            .bind(&event_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(stream_event_count, 1);
    let thread_status: String = sqlx::query_scalar("SELECT status FROM threads WHERE id = ?")
        .bind(&thread)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(thread_status, "cleared");

    let (status, body) = send(
        &app,
        authed_empty(
            "POST",
            &format!("/api/v2/groups/{group}/messages/clear"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cleared_count"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn messages_clear_prevents_next_stream_from_reusing_cleared_thread() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "messages-reuse@example.com").await;
    let owner = owner_id(&state, "messages-reuse@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let old_thread = seed_thread(&state, &group, "active").await;
    seed_message(
        &state,
        &group,
        &old_thread,
        1,
        "visible",
        "user",
        Some(&owner),
        "old",
        None,
    )
    .await;

    let (status, body) = send(
        &app,
        authed_empty(
            "POST",
            &format!("/api/v2/groups/{group}/messages/clear"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cleared_count"].as_u64().unwrap(), 1);

    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content": "fresh"}),
    )
    .await;
    assert_eq!(
        kinds(&events),
        vec![
            "user_message".to_string(),
            "silence".to_string(),
            "done".to_string()
        ]
    );

    let new_thread: String = sqlx::query_scalar(
        "SELECT thread_id FROM messages \
         WHERE group_id = ? AND content = 'fresh' AND status = 'visible'",
    )
    .bind(&group)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_ne!(new_thread, old_thread);
    let old_status: String = sqlx::query_scalar("SELECT status FROM threads WHERE id = ?")
        .bind(&old_thread)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let new_status: String = sqlx::query_scalar("SELECT status FROM threads WHERE id = ?")
        .bind(&new_thread)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(old_status, "cleared");
    assert_eq!(new_status, "active");
}

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
async fn group_stream_supports_gemini_provider_kind_without_network() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "gemini-kind@example.com").await;
    let owner = owner_id(&state, "gemini-kind@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"free_speech": true})).await;

    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Gemini hello\"}]}}]}\n\
                data: [DONE]\n";
    let provider = seed_provider_kind(&state, &owner, "gemini", &fake_provider(body).await).await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Gemini",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content": "hi gemini"}),
    )
    .await;

    let kinds = kinds(&events);
    assert!(kinds.contains(&"agent_message".to_string()));
    assert!(events.iter().any(|event| {
        event["kind"] == "agent_message" && event["payload"]["content"] == "Gemini hello"
    }));
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
