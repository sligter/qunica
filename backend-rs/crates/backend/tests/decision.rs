//! Decision-model integration tests.
//!
//! A local fake decision endpoint answers `POST` with canned System One
//! bodies and records every request, so each test can assert both what the
//! runtime asked and what it did with the answer. No live API is contacted.
//! Every test name contains `decision_` so `--test decision` selects them.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::IntoResponse,
    Router,
};
use qunica_backend::{
    api::{router_with_state_for_tests, AppState},
    decision::{DecisionGate, DecisionScenarios},
    tools::{ApprovalGrants, ToolExecutor, ToolStatus},
};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Fake servers
// ---------------------------------------------------------------------------

/// A decision endpoint that replays `bodies` in order (repeating the last one)
/// and records each request body.
async fn fake_decision_endpoint(bodies: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(VecDeque::from(bodies)));
    let app = Router::new().fallback({
        let requests = Arc::clone(&requests);
        move |request: Request<Body>| {
            let requests = Arc::clone(&requests);
            let queue = Arc::clone(&queue);
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                requests.lock().await.push(parsed);
                let mut queue = queue.lock().await;
                let body = if queue.len() > 1 {
                    queue.pop_front().unwrap()
                } else {
                    queue.front().cloned().unwrap_or_else(|| json!({}))
                };
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    body.to_string(),
                )
                    .into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/decisions"), requests)
}

/// A decision endpoint that always fails with the given status.
async fn failing_decision_endpoint(status: StatusCode) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().fallback(move || async move {
        (status, r#"{"error":{"message":"nope"}}"#).into_response()
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/decisions")
}

/// The headers and body of one request a fake gateway received.
type CapturedRequest = (HashMap<String, String>, Value);

/// A Vercel AI Gateway-style evaluation endpoint. Only the AI SDK route
/// exists, it records every request's headers and body, and it answers with
/// `body` in the AI SDK response shape.
async fn fake_gateway_endpoint(body: Value) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route(
        "/v4/ai/evaluation-model",
        axum::routing::post({
            let requests = Arc::clone(&requests);
            move |request: Request<Body>| {
                let requests = Arc::clone(&requests);
                let body = body.clone();
                async move {
                    let mut headers = request
                        .headers()
                        .iter()
                        .map(|(name, value)| {
                            (name.to_string(), value.to_str().unwrap_or("").to_string())
                        })
                        .collect::<HashMap<_, _>>();
                    // FastAPI-based servers reject a request carrying two
                    // Content-Type headers as non-JSON, so the count matters.
                    headers.insert(
                        "content-type-count".to_string(),
                        request
                            .headers()
                            .get_all(header::CONTENT_TYPE)
                            .iter()
                            .count()
                            .to_string(),
                    );
                    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    requests.lock().await.push((headers, parsed));
                    (
                        [(header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    )
                        .into_response()
                }
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/v4/ai/evaluation-model"), requests)
}

/// An OpenAI-compatible chat endpoint replaying SSE bodies in order.
async fn fake_provider_sequence(bodies: Vec<String>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let queue = Arc::new(Mutex::new(VecDeque::from(bodies)));
    let app = Router::new().fallback(move || {
        let queue = Arc::clone(&queue);
        async move {
            let mut queue = queue.lock().await;
            let body = if queue.len() > 1 {
                queue.pop_front().unwrap()
            } else {
                queue.front().cloned().unwrap_or_default()
            };
            ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn unreachable_local_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

fn text_body(text: &str) -> String {
    format!(
        "data: {}\ndata: [DONE]\n",
        json!({"choices": [{"delta": {"content": text}}]})
    )
}

fn noul_body(answers: &[(&str, f64)]) -> Value {
    let mut map = serde_json::Map::new();
    for (id, probability) in answers {
        map.insert(id.to_string(), json!({"type": "noul", "noul": probability}));
    }
    json!({
        "model": "typesafe/jev-1.13-test",
        "answers": map,
        "usage": {"input_tokens": 30, "output_tokens": 5}
    })
}

fn choice_body(id: &str, choice: &str, confidence: f64) -> Value {
    json!({
        "model": "typesafe/jev-1.13-test",
        "answers": {
            id: {
                "type": "choice",
                "choice": choice,
                "probabilities": { choice: confidence },
                "confidence": confidence
            }
        },
        "usage": {"input_tokens": 30, "output_tokens": 5}
    })
}

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    // Axum's own JSON rejection (an unknown field, say) answers in plain text.
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
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

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
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

async fn owner_id(state: &AppState, email: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
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

async fn seed_provider(state: &AppState, owner_id: &str, base_url: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO llm_providers \
         (id, owner_id, name, kind, base_url, api_key, default_model, reasoning_passback, \
          status, created_at, updated_at) \
         VALUES (?, ?, 'Fake', 'openai-compatible', ?, 'test-key', 'test-model', 0, 'active', \
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

async fn seed_agent(
    state: &AppState,
    owner_id: &str,
    group_id: &str,
    provider_id: &str,
    display_name: &str,
    system_prompt: &str,
    joined_at: &str,
) -> String {
    let agent_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, name, system_prompt, runtime_kind, provider_id, skill_ids_json, \
          status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'llm_chat', ?, '[]', 'active', \
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&agent_id)
    .bind(owner_id)
    .bind(display_name)
    .bind(system_prompt)
    .bind(provider_id)
    .execute(state.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO group_agents \
         (group_id, agent_id, display_name, context_scope_json, status, joined_at, updated_at) \
         VALUES (?, ?, ?, '{\"share_group_workspace\":true}', 'active', ?, ?)",
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

/// Save the account's decision endpoint, then switch the scenarios on for
/// `group`: the connection is global, the switches are per group.
async fn enable_decision(app: &Router, token: &str, group: &str, endpoint: &str, scenarios: Value) {
    let (status, body) = send(
        app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            token,
            json!({
                "decision_endpoint": endpoint,
                "decision_api_key": "test-decision-key",
                "decision_model": "~typesafe/jev-test",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["decision_api_key_configured"], true);
    let (status, body) = send(
        app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group}"),
            token,
            json!({"decision_enabled": true, "decision_scenarios": scenarios}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["decision_enabled"], true);
}

fn stream_uri(group: &str) -> String {
    format!("/api/v2/groups/{group}/messages/stream")
}

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
    let mut events = Vec::new();
    let mut data_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            if !data_lines.is_empty() {
                events.push(serde_json::from_str::<Value>(&data_lines.join("\n")).unwrap());
                data_lines.clear();
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }
    if !data_lines.is_empty() {
        events.push(serde_json::from_str::<Value>(&data_lines.join("\n")).unwrap());
    }
    events
}

fn kinds(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

async fn dispatch_rows(state: &AppState, group_id: &str) -> Vec<(String, String, String)> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT a.name, d.selection_reason, d.status FROM agent_dispatches d \
         JOIN group_turns t ON t.id = d.turn_id \
         JOIN agents a ON a.id = d.target_agent_id \
         WHERE t.group_id = ? \
         ORDER BY d.created_at, d.rowid",
    )
    .bind(group_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// Settings API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn decision_settings_hold_the_connection_and_groups_hold_the_switches() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-settings@example.com").await;

    let (status, body) = send(&app, authed("GET", "/api/v2/settings/system", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["decision_api_key_configured"], false);
    assert_eq!(
        body["decision_endpoint"],
        "https://openrouter.ai/api/alpha/decisions"
    );
    assert_eq!(body["decision_model"], "~typesafe/jev-latest");
    assert_eq!(body["decision_min_confidence"], 0.7);
    assert!(
        body.get("decision_enabled").is_none() && body.get("decision_scenarios").is_none(),
        "the switches are not account-level: {body}"
    );

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"decision_api_key": "sk-or-secret", "decision_min_confidence": 0.5}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["decision_api_key_configured"], true);
    assert!(
        body.get("decision_api_key").is_none(),
        "the key never reads back"
    );
    assert_eq!(body["decision_min_confidence"], 0.5);

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"decision_api_key": null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["decision_api_key_configured"], false);

    for bad in [
        json!({"decision_min_confidence": 1.5}),
        json!({"decision_endpoint": "ftp://nope"}),
    ] {
        let (status, body) = send(
            &app,
            authed_json("PATCH", "/api/v2/settings/system", &token, bad.clone()),
        )
        .await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{bad} -> {status} {body}"
        );
    }

    // Groups: off by default, partial patches, null clears, unknown refused,
    // and a template carries the switches to a new group.
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["decision_enabled"], false);
    for (_, value) in body["decision_scenarios"].as_object().unwrap() {
        assert_eq!(value, &Value::Bool(false));
    }

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group}"),
            &token,
            json!({
                "decision_enabled": true,
                "decision_scenarios": {"shell_risk": true, "reply_outcome": true},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["decision_enabled"], true);
    assert_eq!(body["decision_scenarios"]["shell_risk"], true);
    assert_eq!(body["decision_scenarios"]["reply_outcome"], true);
    assert_eq!(body["decision_scenarios"]["moderator_selection"], false);

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group}"),
            &token,
            json!({"decision_scenarios": {"shell_risk": false}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["decision_scenarios"]["shell_risk"], false);
    assert_eq!(body["decision_scenarios"]["reply_outcome"], true);
    assert_eq!(body["decision_enabled"], true, "an omitted switch is kept");

    let (status, template) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/group-templates",
            &token,
            json!({"name": "Decision template", "group_id": group}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{template}");
    assert_eq!(template["config"]["decision_enabled"], true);
    assert_eq!(
        template["config"]["decision_scenarios"]["reply_outcome"],
        true
    );
    let (status, copied) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Copy", "workspace_id": workspace, "template_id": template["id"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{copied}");
    assert_eq!(copied["decision_enabled"], true);
    assert_eq!(copied["decision_scenarios"]["reply_outcome"], true);
    assert_eq!(copied["decision_scenarios"]["shell_risk"], false);

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group}"),
            &token,
            json!({"decision_scenarios": null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["decision_scenarios"]["reply_outcome"], false);

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group}"),
            &token,
            json!({"decision_scenarios": {"unknown_scenario": true}}),
        ),
    )
    .await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "{status} {body}"
    );
}

#[tokio::test]
async fn decision_test_endpoint_reports_answer_and_failure_without_leaking() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-test@example.com").await;
    let (endpoint, requests) =
        fake_decision_endpoint(vec![noul_body(&[("is_urgent", 0.93)])]).await;

    // Requires a key: none saved, none sent.
    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/settings/decision/test",
            &token,
            json!({"endpoint": endpoint}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/settings/decision/test",
            &token,
            json!({"endpoint": endpoint, "api_key": "sk-or-typed", "model": "~typesafe/jev-x"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["model"], "typesafe/jev-1.13-test");
    assert_eq!(body["sample_probability"], 0.93);
    assert_eq!(body["input_tokens"], 30);
    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["model"], "~typesafe/jev-x");
    assert_eq!(sent[0]["questions"]["is_urgent"]["type"], "noul");
    drop(sent);

    let failing = failing_decision_endpoint(StatusCode::PAYMENT_REQUIRED).await;
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/settings/decision/test",
            &token,
            json!({"endpoint": failing, "api_key": "sk-or-typed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("402"), "{message}");
    assert!(
        message.contains("nope"),
        "the provider's own explanation is what the operator needs: {message}"
    );
    assert!(
        !message.contains("{\"error\""),
        "the raw body must not reach the client: {message}"
    );
}

// ---------------------------------------------------------------------------
// Vercel AI Gateway dialect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn decision_test_endpoint_speaks_the_ai_sdk_gateway_dialect() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-gateway@example.com").await;
    let (endpoint, requests) = fake_gateway_endpoint(json!({
        "answers": { "is_urgent": { "type": "boolean", "probability": 0.91 } },
        "usage": { "inputTokens": 12, "outputTokens": 3 },
        "warnings": []
    }))
    .await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/settings/decision/test",
            &token,
            json!({"endpoint": endpoint, "api_key": "vck_test", "model": "typesafe-ai/jev"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["dialect"], "ai_sdk_gateway");
    assert_eq!(
        body["model"],
        Value::Null,
        "the gateway names no model in its body"
    );
    assert_eq!(body["sample_probability"], 0.91);
    assert_eq!(body["input_tokens"], 12);

    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    let (headers, request) = &sent[0];
    assert_eq!(headers["authorization"], "Bearer vck_test");
    assert_eq!(headers["ai-model-id"], "typesafe-ai/jev");
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(headers["content-type-count"], "1");
    assert_eq!(headers["ai-gateway-protocol-version"], "0.0.1");
    assert_eq!(headers["ai-gateway-auth-method"], "api-key");
    assert_eq!(headers["ai-evaluation-model-specification-version"], "4");
    assert!(
        request.get("model").is_none(),
        "the model travels in a header: {request}"
    );
    assert_eq!(
        request["state"],
        "Help! My payouts have been failing for 3 days."
    );
    assert_eq!(request["questions"]["is_urgent"]["type"], "boolean");
    assert_eq!(
        request["questions"]["is_urgent"]["criteria"]["true"],
        "Explicitly time-sensitive"
    );
}

#[tokio::test]
async fn decision_moderator_selection_through_the_gateway_uses_forwarded_confidence() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-gateway-moderator@example.com").await;
    let owner = owner_id(&state, "decision-gateway-moderator@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let moderator_provider = seed_provider(&state, &owner, &unreachable_local_url().await).await;
    let group = create_group(
        &app,
        &token,
        &workspace,
        json!({
            "free_speech": true,
            "scheduler_enabled": true,
            "max_agent_steps": 1,
            "moderator_enabled": true,
            "moderator_provider_id": moderator_provider,
            "moderator_model": "moderator-model",
        }),
    )
    .await;
    let agent_provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![text_body("<WAITING_FOR_USER> done")]).await,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &agent_provider,
        "Alice",
        "Frontend.",
        "2024-01-01T00:00:00Z",
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &agent_provider,
        "Bob",
        "Backend.",
        "2024-01-02T00:00:00Z",
    )
    .await;

    // The gateway answer carries no `confidence`; TypeSafe's arrives under
    // provider metadata and must be the value the floor is checked against.
    let (endpoint, requests) = fake_gateway_endpoint(json!({
        "answers": {
            "speaker": {
                "type": "choice",
                "choice": "candidate_1",
                "probabilities": { "candidate_0": 0.1, "candidate_1": 0.9 }
            }
        },
        "providerMetadata": { "typesafe": { "confidence": { "speaker": 0.88 } } },
        "usage": { "inputTokens": 40, "outputTokens": 6 }
    }))
    .await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"moderator_selection": true}),
    )
    .await;

    stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "fix the API"}),
    )
    .await;

    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    let (headers, request) = &sent[0];
    assert_eq!(headers["ai-model-id"], "~typesafe/jev-test");
    assert_eq!(request["questions"]["speaker"]["type"], "choice");
    assert_eq!(request["state"]["objective"], "fix the API");
    drop(sent);

    let rows = dispatch_rows(&state, &group).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "Bob");
    assert_eq!(rows[0].1, "decision_model");
    let usage: (String, i64) = sqlx::query_as(
        "SELECT model, total_tokens FROM token_usage_records WHERE owner_id = ? AND agent_name = 'Decision model'",
    )
    .bind(&owner)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(
        usage.0, "~typesafe/jev-test",
        "the gateway names no model, so the configured one is recorded"
    );
    assert_eq!(usage.1, 46);
}

// ---------------------------------------------------------------------------
// Scheduler scenarios
// ---------------------------------------------------------------------------

#[tokio::test]
async fn decision_prefilter_skips_irrelevant_members_and_keeps_mentions() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-prefilter@example.com").await;
    let owner = owner_id(&state, "decision-prefilter@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;
    let provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![text_body("hi")]).await,
    )
    .await;
    let _reviewer = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Reviewer",
        "Reviews code.",
        "2024-01-01T00:00:00Z",
    )
    .await;
    let _designer = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Designer",
        "Designs UI.",
        "2024-01-02T00:00:00Z",
    )
    .await;
    let _writer = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Writer",
        "Writes docs.",
        "2024-01-03T00:00:00Z",
    )
    .await;

    // Reviewer relevant, Designer irrelevant, Writer irrelevant but mentioned.
    let (endpoint, requests) = fake_decision_endpoint(vec![noul_body(&[
        ("member_0", 0.9),
        ("member_1", 0.02),
        ("member_2", 0.01),
    ])])
    .await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"proactive_prefilter": true}),
    )
    .await;

    let events = stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "@Writer please review this diff"}),
    )
    .await;

    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1, "one request screens every member");
    let questions = sent[0]["questions"].as_object().unwrap();
    assert_eq!(questions.len(), 3);
    assert_eq!(
        sent[0]["state"]["latest_message"],
        "@Writer please review this diff"
    );
    assert_eq!(sent[0]["state"]["members"][1]["name"], "Designer");
    drop(sent);

    let warning = events
        .iter()
        .find(|event| {
            event["kind"] == "warning" && event["payload"]["code"] == "decision_prefilter"
        })
        .expect("a prefilter warning names the skipped members");
    assert_eq!(warning["payload"]["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(warning["payload"]["skipped"][0]["display_name"], "Designer");

    let rows = dispatch_rows(&state, &group).await;
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["Writer", "Reviewer"],
        "the mention runs first, Designer never runs"
    );
}

#[tokio::test]
async fn decision_prefilter_failure_dispatches_everyone() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-prefilter-fail@example.com").await;
    let owner = owner_id(&state, "decision-prefilter-fail@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;
    let provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![text_body("hi")]).await,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "A",
        "Agent A.",
        "2024-01-01T00:00:00Z",
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "B",
        "Agent B.",
        "2024-01-02T00:00:00Z",
    )
    .await;
    let endpoint = failing_decision_endpoint(StatusCode::INTERNAL_SERVER_ERROR).await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"proactive_prefilter": true}),
    )
    .await;

    let events = stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "hello"}),
    )
    .await;

    assert!(!kinds(&events).iter().any(|kind| kind == "warning"));
    let rows = dispatch_rows(&state, &group).await;
    assert_eq!(rows.len(), 2, "a failed decision changes nothing");
}

#[tokio::test]
async fn decision_moderator_selection_replaces_the_moderator_call() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-moderator@example.com").await;
    let owner = owner_id(&state, "decision-moderator@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    // The moderator provider is unreachable: if the runtime asked it, the
    // dispatch would be a fallback, not a decision.
    let moderator_provider = seed_provider(&state, &owner, &unreachable_local_url().await).await;
    let group = create_group(
        &app,
        &token,
        &workspace,
        json!({
            "free_speech": true,
            "scheduler_enabled": true,
            "max_agent_steps": 1,
            "moderator_enabled": true,
            "moderator_provider_id": moderator_provider,
            "moderator_model": "moderator-model",
        }),
    )
    .await;
    let agent_provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![text_body("<WAITING_FOR_USER> done")]).await,
    )
    .await;
    let _alice = seed_agent(
        &state,
        &owner,
        &group,
        &agent_provider,
        "Alice",
        "Frontend.",
        "2024-01-01T00:00:00Z",
    )
    .await;
    let bob = seed_agent(
        &state,
        &owner,
        &group,
        &agent_provider,
        "Bob",
        "Backend.",
        "2024-01-02T00:00:00Z",
    )
    .await;

    let (endpoint, requests) =
        fake_decision_endpoint(vec![choice_body("speaker", "candidate_1", 0.92)]).await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"moderator_selection": true}),
    )
    .await;

    stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "fix the API"}),
    )
    .await;

    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["state"]["objective"], "fix the API");
    assert_eq!(sent[0]["state"]["candidates"][1]["name"], "Bob");
    let criteria = &sent[0]["questions"]["speaker"]["criteria"];
    assert!(criteria["candidate_0"]
        .as_str()
        .unwrap()
        .starts_with("Alice"));
    drop(sent);

    let rows = dispatch_rows(&state, &group).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "Bob");
    assert_eq!(rows[0].1, "decision_model");
    let moderator_calls: i64 =
        sqlx::query_scalar("SELECT moderator_calls FROM group_turns WHERE group_id = ?")
            .bind(&group)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(moderator_calls, 0, "the chat moderator was never asked");
    let usage: (String, i64) = sqlx::query_as(
        "SELECT agent_name, total_tokens FROM token_usage_records WHERE owner_id = ? AND agent_name = 'Decision model'",
    )
    .bind(&owner)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(usage.1, 35, "decision usage is recorded");
    let _ = bob;
}

#[tokio::test]
async fn decision_automatic_selects_again_then_finishes_without_chat_moderator() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-automatic@example.com").await;
    let owner = owner_id(&state, "decision-automatic@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let moderator_provider = seed_provider(&state, &owner, &unreachable_local_url().await).await;
    let group = create_group(
        &app,
        &token,
        &workspace,
        json!({
            "free_speech": true,
            "scheduler_mode": "automatic",
            "max_agent_steps": 5,
            "max_steps_per_agent": 3,
            "max_consecutive_failures": 1,
            "max_total_failures": 1,
            "moderator_enabled": true,
            "moderator_provider_id": moderator_provider,
            "moderator_model": "moderator-model",
        }),
    )
    .await;
    let provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![
            text_body("API implemented. Frontend integration remains."),
            text_body("Frontend integration and verification completed."),
        ])
        .await,
    )
    .await;
    for (name, role, joined) in [
        ("Alice", "Frontend.", "2024-01-01T00:00:00Z"),
        ("Bob", "Backend.", "2024-01-02T00:00:00Z"),
    ] {
        seed_agent(&state, &owner, &group, &provider, name, role, joined).await;
    }
    let (endpoint, requests) = fake_decision_endpoint(vec![
        choice_body("speaker", "candidate_1", 0.95),
        noul_body(&[("complete", 0.1)]),
        choice_body("speaker", "candidate_0", 0.95),
        noul_body(&[("complete", 0.95)]),
    ])
    .await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({
            "moderator_selection": true, "automatic_finish": true,
        }),
    )
    .await;
    stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({
            "content": "Implement the API and connect the frontend, then verify both."
        }),
    )
    .await;

    let rows = dispatch_rows(&state, &group).await;
    assert_eq!(rows.len(), 2);
    assert_eq!((&*rows[0].0, &*rows[0].1), ("Bob", "decision_model"));
    assert_eq!((&*rows[1].0, &*rows[1].1), ("Alice", "decision_model"));
    let turn: (String, i64, i64, i64) = sqlx::query_as(
        "SELECT status, moderator_calls, total_failures, total_tokens FROM group_turns WHERE group_id = ?",
    ).bind(&group).fetch_one(state.db.pool()).await.unwrap();
    assert_eq!((&*turn.0, turn.1, turn.2), ("completed", 0, 0));
    assert!(
        turn.3 >= 140,
        "all four decision calls count toward the budget"
    );
    let sent = requests.lock().await;
    assert_eq!(
        sent.len(),
        4,
        "completion takes precedence over another selection"
    );
    assert!(sent[0]["questions"]["speaker"]["criteria"]["defer"].is_string());
    assert!(sent[1]["questions"]["complete"].is_object());
    assert_eq!(sent[2]["state"]["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(
        sent[2]["state"]["candidates"][0]["name"], "Alice",
        "last speaker is excluded"
    );
    assert_eq!(
        sent[2]["questions"]["speaker"]["criteria"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert!(sent[2]["state"]["recent_messages"]
        .to_string()
        .contains("Frontend integration remains"));
}

#[tokio::test]
async fn decision_automatic_selection_falls_back_without_finishing_on_its_own() {
    for (case, answer, enabled) in [
        ("defer", choice_body("speaker", "defer", 0.99), true),
        ("low", choice_body("speaker", "candidate_0", 0.3), true),
        (
            "unknown",
            choice_body("speaker", "candidate_99", 0.99),
            true,
        ),
        ("missing", noul_body(&[]), true),
        (
            "disabled",
            choice_body("speaker", "candidate_0", 0.99),
            false,
        ),
        ("http", json!(null), true),
    ] {
        let (app, state) = router_with_state_for_tests().await;
        let email = format!("decision-auto-{case}@example.com");
        let token = register_and_login(&app, &email).await;
        let owner = owner_id(&state, &email).await;
        let workspace = create_workspace(&app, &token).await;
        let moderator_provider = seed_provider(
            &state,
            &owner,
            &fake_provider_sequence(vec![text_body(
                r#"{"action":"finish","summary":"The objective is already complete."}"#,
            )])
            .await,
        )
        .await;
        let group = create_group(
            &app,
            &token,
            &workspace,
            json!({
                "free_speech": true,
                "scheduler_mode": "automatic",
                "max_agent_steps": 3,
                "moderator_enabled": true,
                "moderator_provider_id": moderator_provider,
                "moderator_model": "moderator-model",
            }),
        )
        .await;
        let provider = seed_provider(&state, &owner, &unreachable_local_url().await).await;
        seed_agent(
            &state,
            &owner,
            &group,
            &provider,
            "Alice",
            "Frontend.",
            "2024-01-01T00:00:00Z",
        )
        .await;
        let (endpoint, requests) = fake_decision_endpoint(vec![answer]).await;
        let endpoint = if case == "http" {
            failing_decision_endpoint(StatusCode::BAD_REQUEST).await
        } else {
            endpoint
        };
        enable_decision(
            &app,
            &token,
            &group,
            &endpoint,
            json!({
                "moderator_selection": enabled,
            }),
        )
        .await;
        stream_events(
            &app,
            &stream_uri(&group),
            &token,
            json!({"content": "Check the objective."}),
        )
        .await;
        assert!(dispatch_rows(&state, &group).await.is_empty(), "{case}");
        let turn: (String, i64, i64) = sqlx::query_as(
            "SELECT status, moderator_calls, total_failures FROM group_turns WHERE group_id = ?",
        )
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
        assert_eq!((&*turn.0, turn.1, turn.2), ("completed", 1, 0), "{case}");
        let sent = requests.lock().await;
        assert_eq!(sent.len(), usize::from(enabled && case != "http"), "{case}");
        for request in sent.iter() {
            assert!(
                request["questions"].get("complete").is_none(),
                "finish switch remains off"
            );
        }
    }
}

#[tokio::test]
async fn decision_automatic_selection_runs_with_finish_disabled_and_respects_token_budget() {
    for (max_tokens, expected_dispatches) in [(10_000, 1), (35, 0)] {
        let (app, state) = router_with_state_for_tests().await;
        let token = register_and_login(&app, "decision-auto-budget@example.com").await;
        let owner = owner_id(&state, "decision-auto-budget@example.com").await;
        let workspace = create_workspace(&app, &token).await;
        let moderator_provider =
            seed_provider(&state, &owner, &unreachable_local_url().await).await;
        let group = create_group(
            &app,
            &token,
            &workspace,
            json!({
                "free_speech": true,
                "scheduler_mode": "automatic",
                "max_agent_steps": 3,
                "max_total_tokens": max_tokens,
                "moderator_enabled": true,
                "moderator_provider_id": moderator_provider,
                "moderator_model": "moderator-model",
            }),
        )
        .await;
        let provider = seed_provider(
            &state,
            &owner,
            &fake_provider_sequence(vec![text_body(
                "<WAITING_FOR_USER> Which frontend should I connect?",
            )])
            .await,
        )
        .await;
        seed_agent(
            &state,
            &owner,
            &group,
            &provider,
            "Alice",
            "Frontend.",
            "2024-01-01T00:00:00Z",
        )
        .await;
        let (endpoint, requests) =
            fake_decision_endpoint(vec![choice_body("speaker", "candidate_0", 0.95)]).await;
        enable_decision(
            &app,
            &token,
            &group,
            &endpoint,
            json!({"moderator_selection": true}),
        )
        .await;
        stream_events(
            &app,
            &stream_uri(&group),
            &token,
            json!({"content": "Connect the frontend."}),
        )
        .await;
        let rows = dispatch_rows(&state, &group).await;
        assert_eq!(rows.len(), expected_dispatches);
        if let Some(row) = rows.first() {
            assert_eq!(row.1, "decision_model");
        }
        let turn: (String, i64) =
            sqlx::query_as("SELECT status, moderator_calls FROM group_turns WHERE group_id = ?")
                .bind(&group)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(turn.1, 0);
        if max_tokens == 35 {
            assert_eq!(turn.0, "budget_exhausted");
        }
        let sent = requests.lock().await;
        assert_eq!(sent.len(), 1);
        assert!(sent[0]["questions"].get("complete").is_none());
    }
}

#[tokio::test]
async fn decision_moderator_selection_low_confidence_falls_back_to_moderator() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-moderator-low@example.com").await;
    let owner = owner_id(&state, "decision-moderator-low@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let moderator_provider = seed_provider(&state, &owner, &unreachable_local_url().await).await;
    let group = create_group(
        &app,
        &token,
        &workspace,
        json!({
            "free_speech": true,
            "scheduler_enabled": true,
            "max_agent_steps": 1,
            "moderator_enabled": true,
            "moderator_provider_id": moderator_provider,
            "moderator_model": "moderator-model",
        }),
    )
    .await;
    let agent_provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![text_body("<WAITING_FOR_USER> done")]).await,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &agent_provider,
        "Alice",
        "Frontend.",
        "2024-01-01T00:00:00Z",
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &agent_provider,
        "Bob",
        "Backend.",
        "2024-01-02T00:00:00Z",
    )
    .await;

    let (endpoint, _) =
        fake_decision_endpoint(vec![choice_body("speaker", "candidate_1", 0.3)]).await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"moderator_selection": true}),
    )
    .await;

    stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "fix the API"}),
    )
    .await;

    let rows = dispatch_rows(&state, &group).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].1, "moderator_fallback",
        "an unsure answer defers to the moderator path"
    );
    let moderator_calls: i64 =
        sqlx::query_scalar("SELECT moderator_calls FROM group_turns WHERE group_id = ?")
            .bind(&group)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(
        moderator_calls, 1,
        "the chat moderator was asked and failed"
    );
}

#[tokio::test]
async fn decision_reply_outcome_ends_the_turn_when_the_agent_asks_the_user() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-reply@example.com").await;
    let owner = owner_id(&state, "decision-reply@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;
    let provider = seed_provider(
        &state,
        &owner,
        &fake_provider_sequence(vec![text_body(
            "Which database should I target, Postgres or SQLite?",
        )])
        .await,
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "A",
        "Agent A.",
        "2024-01-01T00:00:00Z",
    )
    .await;
    seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "B",
        "Agent B.",
        "2024-01-02T00:00:00Z",
    )
    .await;
    let (endpoint, requests) = fake_decision_endpoint(vec![noul_body(&[
        ("waiting_for_user", 0.97),
        ("restated", 0.05),
    ])])
    .await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"reply_outcome": true}),
    )
    .await;

    let events = stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "migrate the schema"}),
    )
    .await;

    let sent = requests.lock().await;
    assert_eq!(
        sent.len(),
        1,
        "only the first reply was assessed before the turn ended"
    );
    assert!(sent[0]["state"]["reply"]
        .as_str()
        .unwrap()
        .contains("Postgres"));
    drop(sent);
    let event_kinds = kinds(&events);
    assert!(
        event_kinds.iter().any(|kind| kind == "waiting_for_user"),
        "{event_kinds:?}"
    );
    let waiting = events
        .iter()
        .find(|event| event["kind"] == "waiting_for_user")
        .unwrap();
    assert_eq!(waiting["payload"]["source"], "decision_model");
    let rows = dispatch_rows(&state, &group).await;
    assert_eq!(rows.len(), 1, "B never ran");
    let status: String = sqlx::query_scalar("SELECT status FROM group_turns WHERE group_id = ?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(status, "waiting_for_user");
}

#[tokio::test]
async fn decision_skill_suggestion_lands_in_the_system_prompt() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register_and_login(&app, "decision-skill@example.com").await;
    let owner = owner_id(&state, "decision-skill@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"proactive_mode": true})).await;

    // A chat provider that records the system prompt it was given.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let prompts = Arc::new(Mutex::new(Vec::<Value>::new()));
    let app_provider = Router::new().fallback({
        let prompts = Arc::clone(&prompts);
        move |request: Request<Body>| {
            let prompts = Arc::clone(&prompts);
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap();
                prompts
                    .lock()
                    .await
                    .push(serde_json::from_slice(&bytes).unwrap());
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    text_body("ok"),
                )
                    .into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app_provider).await.unwrap();
    });
    let provider = seed_provider(&state, &owner, &format!("http://{addr}")).await;
    let agent = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "A",
        "Agent A.",
        "2024-01-01T00:00:00Z",
    )
    .await;

    let (status, skill) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/skills",
            &token,
            json!({
                "name": "pptx-author",
                "description": "Author PowerPoint decks from an outline.",
                "body_markdown": "# pptx-author\nSteps to build a deck.",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{skill}");
    let skill_id = skill["id"].as_str().unwrap();
    sqlx::query("UPDATE agents SET skill_ids_json = ? WHERE id = ?")
        .bind(json!([skill_id]).to_string())
        .bind(&agent)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (endpoint, requests) = fake_decision_endpoint(vec![json!({
        "model": "typesafe/jev-1.13-test",
        "answers": {
            "needs_skill": {"type": "noul", "noul": 0.9},
            "skill": {"type": "choice", "choice": "pptx-author", "probabilities": {"pptx-author": 0.95}, "confidence": 0.95}
        },
        "usage": {"input_tokens": 20, "output_tokens": 4}
    })])
    .await;
    enable_decision(
        &app,
        &token,
        &group,
        &endpoint,
        json!({"skill_suggestion": true}),
    )
    .await;

    stream_events(
        &app,
        &stream_uri(&group),
        &token,
        json!({"content": "make me a pitch deck"}),
    )
    .await;

    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["state"]["request"], "make me a pitch deck");
    assert_eq!(sent[0]["state"]["skills"][0]["name"], "pptx-author");
    drop(sent);
    let prompts = prompts.lock().await;
    let system = prompts[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("Skill likely relevant to the current request: pptx-author"),
        "{system}"
    );
}

// ---------------------------------------------------------------------------
// Tool scenarios (executor level, no HTTP app)
// ---------------------------------------------------------------------------

fn score_body(id: &str, score: f64, confidence: f64, hazards: &[(&str, f64)]) -> Value {
    let mut answers = serde_json::Map::new();
    answers.insert(
        id.to_string(),
        json!({"type": "score", "score": score, "legend": {}, "probabilities": {}, "confidence": confidence}),
    );
    for (hazard, probability) in hazards {
        answers.insert(
            hazard.to_string(),
            json!({"type": "noul", "noul": probability}),
        );
    }
    json!({"model": "typesafe/jev-1.13-test", "answers": answers, "usage": {"input_tokens": 10, "output_tokens": 2}})
}

#[tokio::test]
async fn decision_shell_risk_escalates_an_allowed_command_to_approval() {
    let root = tempfile::tempdir().unwrap();
    let (endpoint, requests) = fake_decision_endpoint(vec![score_body(
        "destructiveness",
        2.4,
        0.9,
        &[
            ("deletes_files", 0.95),
            ("writes_outside_workspace", 0.1),
            ("discards_git_work", 0.05),
        ],
    )])
    .await;
    let gate = DecisionGate::for_tests(
        &endpoint,
        DecisionScenarios {
            shell_risk: true,
            ..Default::default()
        },
        0.7,
    );
    let executor = ToolExecutor::new(Some(root.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate.clone()));

    // `python -c` passes the regex policy; the model flags it.
    let result = executor
        .execute(
            "Bash",
            json!({"command": "python -c \"import shutil; shutil.rmtree('build')\""}),
        )
        .await;
    assert_eq!(
        result.status,
        ToolStatus::ApprovalRequired,
        "{}",
        result.output
    );
    let payload: Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(payload["approval_request"]["rule"], "decision-risk");
    assert_eq!(payload["approval_request"]["tool_name"], "Bash");
    let reason = payload["approval_request"]["reason"].as_str().unwrap();
    assert!(
        reason.contains("delete or overwrite existing files"),
        "{reason}"
    );
    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    assert!(sent[0]["state"]["command"]
        .as_str()
        .unwrap()
        .contains("rmtree"));
    assert_eq!(sent[0]["questions"]["destructiveness"]["type"], "score");
    drop(sent);
    assert_eq!(
        gate.take_usage().len(),
        1,
        "tool-level calls are collected for accounting"
    );

    // A remembered grant for the rule skips the second opinion.
    let mut grants = ApprovalGrants::default();
    grants.grant("decision-risk");
    let granted = ToolExecutor::new(Some(root.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate.clone()))
        .with_approvals(grants)
        .execute("Bash", json!({"command": "echo granted"}))
        .await;
    assert_ne!(
        granted.status,
        ToolStatus::ApprovalRequired,
        "{}",
        granted.output
    );
    assert_eq!(requests.lock().await.len(), 1, "no second request was made");
}

#[tokio::test]
async fn decision_shell_risk_never_relaxes_the_policy_and_tolerates_failure() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("victim.txt"), "keep me").unwrap();
    // The model says "read-only", but the policy asks anyway: the policy wins.
    let (benign, requests) =
        fake_decision_endpoint(vec![score_body("destructiveness", 0.1, 0.99, &[])]).await;
    let gate = DecisionGate::for_tests(
        &benign,
        DecisionScenarios {
            shell_risk: true,
            ..Default::default()
        },
        0.7,
    );
    let executor = ToolExecutor::new(Some(root.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate));
    let result = executor
        .execute("Bash", json!({"command": "rm victim.txt"}))
        .await;
    assert_eq!(result.status, ToolStatus::ApprovalRequired);
    let payload: Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(payload["approval_request"]["rule"], "delete-files");
    assert!(root.path().join("victim.txt").exists());
    assert!(
        requests.lock().await.is_empty(),
        "a policy verdict is never second-guessed"
    );

    // A benign verdict lets an allowed command run.
    let result = executor
        .execute("Bash", json!({"command": "echo fine"}))
        .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert_eq!(requests.lock().await.len(), 1);

    // A failing endpoint is "no opinion": the allowed command still runs.
    let failing = failing_decision_endpoint(StatusCode::BAD_GATEWAY).await;
    let gate = DecisionGate::for_tests(
        &failing,
        DecisionScenarios {
            shell_risk: true,
            ..Default::default()
        },
        0.7,
    );
    let result = ToolExecutor::new(Some(root.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate))
        .execute("Bash", json!({"command": "echo still fine"}))
        .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);

    // A gate whose shell scenario is off is never consulted.
    let (endpoint, requests) =
        fake_decision_endpoint(vec![score_body("destructiveness", 3.0, 0.99, &[])]).await;
    let gate = DecisionGate::for_tests(
        &endpoint,
        DecisionScenarios {
            note_validation: true,
            ..Default::default()
        },
        0.7,
    );
    let result = ToolExecutor::new(Some(root.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate))
        .execute("Bash", json!({"command": "echo off"}))
        .await;
    assert_eq!(result.status, ToolStatus::Completed);
    assert!(requests.lock().await.is_empty());
}

#[tokio::test]
async fn decision_note_validation_refuses_implemented_without_evidence() {
    let workspace = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    let note = "# Cache policy\nStatus: proposed\nSince: 2026-09-01\nCategory: 决策\n\n## Problem\nSlow.\n\n## Decision\nWe agree to add a cache.\n\n## Alternatives considered\nNone.\n\n## Consequences\nFaster.\n";
    std::fs::write(notes.path().join("cache.md"), note).unwrap();

    let (endpoint, requests) =
        fake_decision_endpoint(vec![noul_body(&[("cites_evidence", 0.1)])]).await;
    let gate = DecisionGate::for_tests(
        &endpoint,
        DecisionScenarios {
            note_validation: true,
            ..Default::default()
        },
        0.7,
    );
    let executor = ToolExecutor::new(Some(workspace.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate.clone()))
        .with_group_notes(Some(notes.path().to_path_buf()))
        .unwrap();

    // Promoting to implemented with only "we agree" in Decision is refused.
    let result = executor
        .execute(
            "EditGroupNote",
            json!({"path": "cache.md", "oldText": "Status: proposed", "newText": "Status: implemented"}),
        )
        .await;
    assert_eq!(result.status, ToolStatus::Failed, "{}", result.output);
    assert!(result.output.contains("implemented"), "{}", result.output);
    assert!(std::fs::read_to_string(notes.path().join("cache.md"))
        .unwrap()
        .contains("Status: proposed"));
    let sent = requests.lock().await;
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]["state"]["note"]
            .as_str()
            .unwrap()
            .contains("Status: implemented"),
        "the model sees the edited note"
    );
    drop(sent);

    // An edit that does not claim implemented is not even asked about.
    let result = executor
        .execute(
            "EditGroupNote",
            json!({"path": "cache.md", "oldText": "Slow.", "newText": "Pages load slowly."}),
        )
        .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert_eq!(requests.lock().await.len(), 1);

    // With evidence the promotion goes through.
    let (endpoint, _) = fake_decision_endpoint(vec![noul_body(&[("cites_evidence", 0.9)])]).await;
    let gate = DecisionGate::for_tests(
        &endpoint,
        DecisionScenarios {
            note_validation: true,
            ..Default::default()
        },
        0.7,
    );
    let executor = ToolExecutor::new(Some(workspace.path().to_path_buf()))
        .unwrap()
        .with_decision(Some(gate))
        .with_group_notes(Some(notes.path().to_path_buf()))
        .unwrap();
    let result = executor
        .execute(
            "EditGroupNote",
            json!({"path": "cache.md", "oldText": "Status: proposed", "newText": "Status: implemented"}),
        )
        .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(std::fs::read_to_string(notes.path().join("cache.md"))
        .unwrap()
        .contains("Status: implemented"));
}
