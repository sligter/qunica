//! Staged configuration changes and the approval endpoint.
//!
//! The whole point of this surface is that a tool call changes nothing. These
//! tests pin that: proposing stages a row and leaves the database alone,
//! approval is the only thing that applies it, approval is idempotent-safe and
//! owner-scoped, and the forbidden targets are refused when they are proposed
//! rather than when they are approved.

use std::{sync::Arc, time::Duration};

use ag_swarmer_backend::{
    api::{router_with_state_for_tests, AppState},
    tools::{AppControlContext, ToolExecutor, ToolResult, ToolStatus},
};
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tower::ServiceExt;

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

fn request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn register(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        request(
            "POST",
            "/api/v2/auth/register",
            None,
            json!({"email": email, "password": "supersecret", "name": "Tester"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/auth/login",
            None,
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["access_token"].as_str().unwrap().to_string()
}

async fn owner_id(state: &AppState, email: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
}

async fn create_workspace(app: &Router, token: &str) -> String {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/workspaces",
            Some(token),
            json!({"name": "Workspace", "backend_type": "cloud_sandbox"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    body["id"].as_str().unwrap().to_string()
}

async fn controlled_provider(text: &'static str) -> (String, Arc<Notify>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let release = Arc::new(Notify::new());
    let app = Router::new().fallback({
        let release = release.clone();
        move || {
            let release = release.clone();
            async move {
                release.notified().await;
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    format!(
                        "data: {}\ndata: [DONE]\n",
                        json!({"choices": [{"delta": {"content": text}}]})
                    ),
                )
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), release)
}

fn executor_for(state: &AppState, owner_id: &str) -> ToolExecutor {
    ToolExecutor::without_workspace().with_app_control(AppControlContext::new(
        state.db.pool().clone(),
        owner_id.to_string(),
        uuid::Uuid::new_v4().to_string(),
    ))
}

fn action_id_of(result: &ToolResult) -> String {
    let value: Value = serde_json::from_str(&result.output)
        .unwrap_or_else(|_| panic!("not JSON: {}", result.output));
    value["action_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no action_id in {}", result.output))
        .to_string()
}

async fn stage_and_approve(
    app: &Router,
    token: &str,
    executor: &ToolExecutor,
    payload: Value,
) -> ToolResult {
    let staged = executor.execute("AppPropose", payload).await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );
    let action = action_id_of(&staged);
    let (status, body) = send(
        app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    staged
}

async fn count(state: &AppState, table: &str, owner_id: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE owner_id = ?");
    sqlx::query_scalar::<_, i64>(&sql)
        .bind(owner_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
}

async fn pending_count(state: &AppState, owner_id: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM app_actions WHERE owner_id = ? AND status = 'pending'",
    )
    .bind(owner_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap()
}

async fn wait_until_applied(state: &AppState, action_id: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status: String = sqlx::query_scalar("SELECT status FROM app_actions WHERE id = ?")
                .bind(action_id)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            match status.as_str() {
                "applied" => break,
                "failed" => panic!("background action failed"),
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .expect("background action did not finish");
}

#[tokio::test]
async fn proposing_stages_a_row_and_changes_nothing() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "stage-only@example.com").await;
    let owner = owner_id(&state, "stage-only@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let executor = executor_for(&state, &owner);

    let before = count(&state, "agents", &owner).await;
    let result = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "agent",
                "action": "create",
                "payload": {"name": "Researcher", "workspace_id": workspace}
            }),
        )
        .await;

    assert_eq!(
        result.status,
        ToolStatus::ApprovalRequired,
        "{}",
        result.output
    );
    assert_eq!(count(&state, "agents", &owner).await, before);
    assert_eq!(pending_count(&state, &owner).await, 1);
    // The summary is what the user reads on the card, so it has to name the
    // thing being created.
    assert!(result.output.contains("Researcher"), "{}", result.output);
}

#[tokio::test]
async fn only_approval_applies_a_staged_action_and_only_once() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "approve-once@example.com").await;
    let owner = owner_id(&state, "approve-once@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let executor = executor_for(&state, &owner);

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "agent",
                "action": "create",
                "payload": {"name": "Researcher", "workspace_id": workspace}
            }),
        )
        .await;
    let action = action_id_of(&staged);
    assert_eq!(count(&state, "agents", &owner).await, 0);

    let uri = format!("/api/v2/app-actions/{action}/approve");
    let (first, second) = tokio::join!(
        send(&app, request("POST", &uri, Some(&token), json!({}))),
        send(&app, request("POST", &uri, Some(&token), json!({}))),
    );
    for (status, body) in [first, second] {
        assert_eq!(status, StatusCode::OK, "body: {body:?}");
        assert!(
            matches!(body["status"].as_str(), Some("approved" | "applied")),
            "body: {body:?}"
        );
    }
    assert_eq!(count(&state, "agents", &owner).await, 1);

    // A later retry is idempotent and reports the durable terminal state.
    let (status, body) = send(&app, request("POST", &uri, Some(&token), json!({}))).await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["status"], "applied");
    assert_eq!(count(&state, "agents", &owner).await, 1);
}

#[tokio::test]
async fn chat_proposals_create_a_private_chat_and_send_messages() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "chat-actions@example.com").await;
    let owner = owner_id(&state, "chat-actions@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let (provider_url, release_provider) = controlled_provider("Hello from Solo").await;
    let (status, provider) = send(
        &app,
        request(
            "POST",
            "/api/v2/llm-providers",
            Some(&token),
            json!({
                "name": "Fake",
                "kind": "openai-compatible",
                "base_url": provider_url,
                "api_key": "test-key",
                "default_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {provider:?}");
    let provider_id = provider["id"].as_str().unwrap().to_string();
    let executor = executor_for(&state, &owner);
    stage_and_approve(
        &app,
        &token,
        &executor,
        json!({
            "target_kind": "agent",
            "action": "create",
            "payload": {
                "name": "Solo",
                "workspace_id": workspace,
                "runtime_kind": "llm_chat",
                "provider_id": provider_id,
                "llm_config": {"model": "test-model"}
            }
        }),
    )
    .await;
    let agent_id: String = sqlx::query_scalar("SELECT id FROM agents WHERE name = 'Solo'")
        .fetch_one(state.db.pool())
        .await
        .unwrap();

    let inspected = executor
        .execute("AppGet", json!({"kind": "agent", "id": agent_id}))
        .await;
    assert_eq!(
        inspected.status,
        ToolStatus::Completed,
        "{}",
        inspected.output
    );
    let inspected: Value = serde_json::from_str(&inspected.output).unwrap();
    assert_eq!(inspected["item"]["llm_provider_id"], provider_id);
    assert_eq!(inspected["item"]["llm_config"]["model"], "test-model");
    assert!(inspected["item"].get("provider_id").is_none());

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "chat",
                "action": "create",
                "payload": {"agent_id": agent_id, "message": "What is in your workspace?"}
            }),
        )
        .await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );
    assert!(staged.output.contains("Solo"), "{}", staged.output);
    assert_eq!(count(&state, "groups", &owner).await, 0);

    let action = action_id_of(&staged);
    let (status, body) = tokio::time::timeout(
        Duration::from_secs(1),
        send(
            &app,
            request(
                "POST",
                &format!("/api/v2/app-actions/{action}/approve"),
                Some(&token),
                json!({}),
            ),
        ),
    )
    .await
    .expect("approval waited for the agent's reply");
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["status"], "approved");
    release_provider.notify_one();
    wait_until_applied(&state, &action).await;

    let chat_id: String = sqlx::query_scalar(
        "SELECT id FROM groups WHERE owner_id = ? AND conversation_kind = 'direct'",
    )
    .bind(&owner)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    let first: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id = ? AND sender_type = 'user' ORDER BY seq",
    )
    .bind(&chat_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(first, "What is in your workspace?");
    let first_reply: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id = ? AND sender_type = 'agent' ORDER BY seq",
    )
    .bind(&chat_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(first_reply, "Hello from Solo");

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "chat",
                "action": "update",
                "target_id": chat_id,
                "payload": {"message": "Please summarize it."}
            }),
        )
        .await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );
    let action = action_id_of(&staged);
    let (status, body) = tokio::time::timeout(
        Duration::from_secs(1),
        send(
            &app,
            request(
                "POST",
                &format!("/api/v2/app-actions/{action}/approve"),
                Some(&token),
                json!({}),
            ),
        ),
    )
    .await
    .expect("approval waited for the agent's reply");
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["status"], "approved");
    release_provider.notify_one();
    wait_until_applied(&state, &action).await;

    let messages: Vec<String> = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id = ? AND sender_type = 'user' ORDER BY seq",
    )
    .bind(&chat_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(
        messages,
        ["What is in your workspace?", "Please summarize it."]
    );
    let replies: Vec<String> = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id = ? AND sender_type = 'agent' ORDER BY seq",
    )
    .bind(&chat_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(replies, ["Hello from Solo", "Hello from Solo"]);
}

#[tokio::test]
async fn group_proposals_create_a_group_and_send_messages() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "group-actions@example.com").await;
    let owner = owner_id(&state, "group-actions@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let executor = executor_for(&state, &owner);

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "group",
                "action": "create",
                "payload": {
                    "name": "Team",
                    "workspace_id": workspace,
                    "message": "Start the discussion."
                }
            }),
        )
        .await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );
    assert!(staged.output.contains("Team"), "{}", staged.output);
    assert!(
        staged.output.contains("Start the discussion."),
        "{}",
        staged.output
    );
    assert_eq!(count(&state, "groups", &owner).await, 0);

    let action = action_id_of(&staged);
    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    wait_until_applied(&state, &action).await;

    let group_id: String = sqlx::query_scalar(
        "SELECT id FROM groups WHERE owner_id = ? AND conversation_kind = 'group'",
    )
    .bind(&owner)
    .fetch_one(state.db.pool())
    .await
    .unwrap();

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "group",
                "action": "update",
                "target_id": group_id,
                "payload": {"message": "Please continue."}
            }),
        )
        .await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );
    assert!(
        staged.output.contains("Please continue."),
        "{}",
        staged.output
    );
    let action = action_id_of(&staged);
    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    wait_until_applied(&state, &action).await;

    let messages: Vec<String> = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id = ? AND sender_type = 'user' ORDER BY seq",
    )
    .bind(&group_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(messages, ["Start the discussion.", "Please continue."]);

    let owner_removal = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "group",
                "action": "update",
                "target_id": group_id,
                "payload": {"membership": {
                    "operation": "remove_user",
                    "email": "group-actions@example.com"
                }}
            }),
        )
        .await;
    assert_eq!(
        owner_removal.status,
        ToolStatus::Failed,
        "{}",
        owner_removal.output
    );

    let (status, agent) = send(
        &app,
        request(
            "POST",
            "/api/v2/agents",
            Some(&token),
            json!({"name": "Researcher", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {agent:?}");
    let agent_id = agent["id"].as_str().unwrap().to_string();
    register(&app, "colleague@example.com").await;

    let staged = stage_and_approve(
        &app,
        &token,
        &executor,
        json!({
            "target_kind": "group",
            "action": "update",
            "target_id": group_id,
            "payload": {"membership": {"operation": "add_agent", "agent_id": agent_id}}
        }),
    )
    .await;
    assert!(staged.output.contains("Researcher"), "{}", staged.output);

    let staged = stage_and_approve(
        &app,
        &token,
        &executor,
        json!({
            "target_kind": "group",
            "action": "update",
            "target_id": group_id,
            "payload": {"membership": {
                "operation": "add_user",
                "email": "COLLEAGUE@example.com"
            }}
        }),
    )
    .await;
    assert!(
        staged.output.contains("colleague@example.com"),
        "{}",
        staged.output
    );

    let inspected = executor
        .execute("AppGet", json!({"kind": "group", "id": group_id}))
        .await;
    assert_eq!(
        inspected.status,
        ToolStatus::Completed,
        "{}",
        inspected.output
    );
    let inspected: Value = serde_json::from_str(&inspected.output).unwrap();
    assert_eq!(inspected["item"]["agents"][0]["id"], agent_id);
    assert!(inspected["item"]["users"]
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["email"] == "colleague@example.com"));

    stage_and_approve(
        &app,
        &token,
        &executor,
        json!({
            "target_kind": "group",
            "action": "update",
            "target_id": group_id,
            "payload": {"membership": {"operation": "remove_agent", "agent_id": agent_id}}
        }),
    )
    .await;
    stage_and_approve(
        &app,
        &token,
        &executor,
        json!({
            "target_kind": "group",
            "action": "update",
            "target_id": group_id,
            "payload": {"membership": {
                "operation": "remove_user",
                "email": "colleague@example.com"
            }}
        }),
    )
    .await;

    let active_agents: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_agents WHERE group_id = ? AND status = 'active'",
    )
    .bind(&group_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    let active_colleagues: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members gm JOIN users u ON u.id = gm.user_id \
         WHERE gm.group_id = ? AND gm.status = 'active' AND u.email = ?",
    )
    .bind(&group_id)
    .bind("colleague@example.com")
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!((active_agents, active_colleagues), (0, 0));
}

#[tokio::test]
async fn rejecting_is_terminal() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "reject@example.com").await;
    let owner = owner_id(&state, "reject@example.com").await;
    let executor = executor_for(&state, &owner);

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "skill",
                "action": "create",
                "payload": {"name": "Review", "body_markdown": "# Review"}
            }),
        )
        .await;
    let action = action_id_of(&staged);

    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/reject"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["status"], "rejected");

    let (status, _) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(count(&state, "skills", &owner).await, 0);
}

#[tokio::test]
async fn another_user_cannot_approve_or_even_see_a_staged_action() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "owner-action@example.com").await;
    let stranger = register(&app, "stranger-action@example.com").await;
    let owner = owner_id(&state, "owner-action@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let executor = executor_for(&state, &owner);

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "agent",
                "action": "create",
                "payload": {"name": "Researcher", "workspace_id": workspace}
            }),
        )
        .await;
    let action = action_id_of(&staged);

    let (status, _) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&stranger),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/app-actions/{action}"),
            &stranger,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(count(&state, "agents", &owner).await, 0);

    let (status, list) = send(&app, authed("GET", "/api/v2/app-actions", &stranger)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn forbidden_targets_are_refused_when_proposed() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "forbidden@example.com").await;
    let owner = owner_id(&state, "forbidden@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let (_, agent) = send(
        &app,
        request(
            "POST",
            "/api/v2/agents",
            Some(&token),
            json!({"name": "Victim", "workspace_id": workspace}),
        ),
    )
    .await;
    let agent_id = agent["id"].as_str().unwrap();
    let executor = executor_for(&state, &owner);

    for payload in [
        // A provider API key must never be settable by the model.
        json!({
            "target_kind": "provider", "action": "create",
            "payload": {"name": "P", "kind": "openai-compatible", "api_key": "sk-x",
                        "base_url": "https://x.invalid/v1", "default_model": "m"}
        }),
        // An stdio MCP server launches a local process.
        json!({
            "target_kind": "mcp", "action": "create",
            "payload": {"name": "Local", "transport": "stdio", "command": "node"}
        }),
        // Deletion is out of scope entirely.
        json!({"target_kind": "agent", "action": "delete", "target_id": agent_id}),
        json!({"target_kind": "workspace", "action": "delete", "target_id": workspace}),
        // Unknown kinds and actions fall through the allowlist.
        json!({"target_kind": "user", "action": "create", "payload": {}}),
        json!({"target_kind": "agent", "action": "exfiltrate", "payload": {}}),
    ] {
        let result = executor.execute("AppPropose", payload.clone()).await;
        assert_eq!(
            result.status,
            ToolStatus::Failed,
            "should have been refused: {payload}"
        );
    }

    // Nothing was staged, so nothing can later be approved into existence.
    assert_eq!(pending_count(&state, &owner).await, 0);
}

#[tokio::test]
async fn an_invalid_payload_fails_at_propose_time() {
    let (app, state) = router_with_state_for_tests().await;
    register(&app, "invalid-payload@example.com").await;
    let owner = owner_id(&state, "invalid-payload@example.com").await;
    let executor = executor_for(&state, &owner);

    // Staging a proposal that cannot apply would show the user a card that
    // only fails once they trust it.
    for payload in [
        json!({"target_kind": "agent", "action": "create", "payload": {"name": ""}}),
        json!({"target_kind": "agent", "action": "create",
               "payload": {"name": "X", "workspace_id": "not-a-uuid"}}),
        json!({"target_kind": "skill", "action": "create", "payload": {"name": "X"}}),
        json!({"target_kind": "group", "action": "create",
               "payload": {"name": "G", "communication_mode": "telepathy"}}),
        json!({"target_kind": "group", "action": "update",
               "target_id": "00000000-0000-4000-8000-000000000001",
               "payload": {"message": " "}}),
    ] {
        let result = executor.execute("AppPropose", payload.clone()).await;
        assert_eq!(result.status, ToolStatus::Failed, "accepted: {payload}");
    }
    assert_eq!(pending_count(&state, &owner).await, 0);
}

#[tokio::test]
async fn an_update_proposal_applies_to_the_named_row() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "update-apply@example.com").await;
    let owner = owner_id(&state, "update-apply@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let (_, agent) = send(
        &app,
        request(
            "POST",
            "/api/v2/agents",
            Some(&token),
            json!({"name": "Before", "workspace_id": workspace}),
        ),
    )
    .await;
    let agent_id = agent["id"].as_str().unwrap().to_string();
    let executor = executor_for(&state, &owner);

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "agent",
                "action": "update",
                "target_id": agent_id,
                "payload": {"name": "After"}
            }),
        )
        .await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );

    let action = action_id_of(&staged);
    let (status, _) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, after) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(after["name"], "After");
}

#[tokio::test]
async fn an_update_cannot_retarget_another_owners_row() {
    let (app, state) = router_with_state_for_tests().await;
    register(&app, "retarget-owner@example.com").await;
    let stranger = register(&app, "retarget-stranger@example.com").await;
    let owner = owner_id(&state, "retarget-owner@example.com").await;
    let stranger_workspace = create_workspace(&app, &stranger).await;
    let (_, victim) = send(
        &app,
        request(
            "POST",
            "/api/v2/agents",
            Some(&stranger),
            json!({"name": "TheirAgent", "workspace_id": stranger_workspace}),
        ),
    )
    .await;
    let victim_id = victim["id"].as_str().unwrap().to_string();

    let executor = executor_for(&state, &owner);
    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "agent",
                "action": "update",
                "target_id": victim_id,
                "payload": {"name": "Hijacked"}
            }),
        )
        .await;
    assert_eq!(staged.status, ToolStatus::Failed, "{}", staged.output);

    let (_, after) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{victim_id}"), &stranger),
    )
    .await;
    assert_eq!(after["name"], "TheirAgent");

    let (status, foreign_chat) = send(
        &app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(&stranger),
            json!({"agent_id": victim_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{foreign_chat}");
    let foreign_chat_id = foreign_chat["id"].as_str().unwrap();

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "chat",
                "action": "update",
                "target_id": foreign_chat_id,
                "payload": {"message": "Cross-account message"}
            }),
        )
        .await;
    assert_eq!(staged.status, ToolStatus::Failed, "{}", staged.output);

    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ?")
        .bind(foreign_chat_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(message_count, 0);
}

#[tokio::test]
async fn a_failed_apply_records_why_and_does_not_stay_pending() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "apply-fails@example.com").await;
    let owner = owner_id(&state, "apply-fails@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let executor = executor_for(&state, &owner);

    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "agent",
                "action": "create",
                "payload": {"name": "Researcher", "workspace_id": workspace}
            }),
        )
        .await;
    let action = action_id_of(&staged);

    // The workspace disappears between proposal and approval, so the apply
    // fails on validation the core still performs.
    sqlx::query("UPDATE workspaces SET status = 'deleted' WHERE id = ?")
        .bind(&workspace)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body:?}");
    assert_eq!(count(&state, "agents", &owner).await, 0);

    // The row must leave `pending`, with the reason kept for the history page.
    let (row_status, result_json): (String, Option<String>) =
        sqlx::query_as("SELECT status, result_json FROM app_actions WHERE id = ?")
            .bind(&action)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(row_status, "failed");
    assert!(result_json.unwrap_or_default().contains("workspace"));
}

#[tokio::test]
async fn prefill_returns_a_route_and_stages_nothing() {
    let (app, state) = router_with_state_for_tests().await;
    register(&app, "prefill@example.com").await;
    let owner = owner_id(&state, "prefill@example.com").await;
    let executor = executor_for(&state, &owner);

    let result = executor
        .execute(
            "AppPrefill",
            json!({"target_kind": "provider", "action": "create",
                   "fields": {"name": "OpenAI", "kind": "openai-compatible"}}),
        )
        .await;

    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    let value: Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(value["route"], "/providers/new");
    assert_eq!(value["fields"]["name"], "OpenAI");
    assert_eq!(pending_count(&state, &owner).await, 0);
}

#[tokio::test]
async fn the_action_list_shows_history_newest_first() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "history@example.com").await;
    let owner = owner_id(&state, "history@example.com").await;
    let executor = executor_for(&state, &owner);

    for name in ["First", "Second"] {
        executor
            .execute(
                "AppPropose",
                json!({
                    "target_kind": "skill",
                    "action": "create",
                    "payload": {"name": name, "body_markdown": "# Body"}
                }),
            )
            .await;
    }

    let (status, list) = send(&app, authed("GET", "/api/v2/app-actions?limit=1", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["has_more"], true);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    for item in items {
        assert_eq!(item["status"], "pending");
        assert_eq!(item["target_kind"], "skill");
        assert!(item["summary"].as_str().is_some_and(|s| !s.is_empty()));
    }

    let (status, list) = send(
        &app,
        authed("GET", "/api/v2/app-actions?limit=1&skip=1", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["has_more"], false);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn only_resolved_action_history_can_be_deleted() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "delete-history@example.com").await;
    let owner = owner_id(&state, "delete-history@example.com").await;
    let executor = executor_for(&state, &owner);
    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "skill",
                "action": "create",
                "payload": {"name": "History", "body_markdown": "# Body"}
            }),
        )
        .await;
    let action = action_id_of(&staged);
    let action_uri = format!("/api/v2/app-actions/{action}");

    let (status, _) = send(&app, authed("DELETE", &action_uri, &token)).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = send(
        &app,
        request(
            "POST",
            &format!("{action_uri}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app, authed("DELETE", &action_uri, &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, list) = send(&app, authed("GET", "/api/v2/app-actions", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn clearing_history_keeps_unfinished_and_other_users_actions() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "clear-history@example.com").await;
    let owner = owner_id(&state, "clear-history@example.com").await;
    let executor = executor_for(&state, &owner);
    let other_token = register(&app, "other-history@example.com").await;
    let other_owner = owner_id(&state, "other-history@example.com").await;
    let other_executor = executor_for(&state, &other_owner);

    let pending = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "skill",
                "action": "create",
                "payload": {"name": "Pending history", "body_markdown": "# Body"}
            }),
        )
        .await;
    assert_eq!(pending.status, ToolStatus::ApprovalRequired);
    stage_and_approve(
        &app,
        &token,
        &executor,
        json!({
            "target_kind": "skill",
            "action": "create",
            "payload": {"name": "Applied history", "body_markdown": "# Body"}
        }),
    )
    .await;
    stage_and_approve(
        &app,
        &other_token,
        &other_executor,
        json!({
            "target_kind": "skill",
            "action": "create",
            "payload": {"name": "Other history", "body_markdown": "# Body"}
        }),
    )
    .await;

    let (status, _) = send(&app, authed("DELETE", "/api/v2/app-actions", &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, owner_list) = send(&app, authed("GET", "/api/v2/app-actions", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let owner_items = owner_list["items"].as_array().unwrap();
    assert_eq!(owner_items.len(), 1);
    assert_eq!(owner_items[0]["status"], "pending");

    let (status, other_list) = send(&app, authed("GET", "/api/v2/app-actions", &other_token)).await;
    assert_eq!(status, StatusCode::OK);
    let other_items = other_list["items"].as_array().unwrap();
    assert_eq!(other_items.len(), 1);
    assert_eq!(other_items[0]["status"], "applied");
}

#[tokio::test]
async fn propose_is_unavailable_without_an_app_control_context() {
    let executor = ToolExecutor::without_workspace();
    for name in ["AppPropose", "AppPrefill"] {
        let result = executor
            .execute(
                name,
                json!({"target_kind": "agent", "action": "create", "payload": {}}),
            )
            .await;
        assert_eq!(result.status, ToolStatus::SetupRequired, "{name}");
    }
}

#[tokio::test]
async fn a_workspace_can_be_proposed_with_auto_create() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "auto-create@example.com").await;
    let owner = owner_id(&state, "auto-create@example.com").await;
    let root = tempfile::tempdir().unwrap();

    // `auto_create` needs a configured root to create the folder under.
    let (status, _) = send(
        &app,
        request(
            "PATCH",
            "/api/v2/settings/system",
            Some(&token),
            json!({ "group_workspace_root": root.path().to_string_lossy() }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let executor = executor_for(&state, &owner);
    let staged = executor
        .execute(
            "AppPropose",
            json!({
                "target_kind": "workspace",
                "action": "create",
                "payload": {"name": "Scratch", "backend_type": "local", "auto_create": true}
            }),
        )
        .await;
    assert_eq!(
        staged.status,
        ToolStatus::ApprovalRequired,
        "{}",
        staged.output
    );

    let action = action_id_of(&staged);
    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");

    // The point of auto_create: a real directory exists afterwards, so the user
    // never has to go find or type a path.
    let local_path: Option<String> =
        sqlx::query_scalar("SELECT local_path FROM workspaces WHERE owner_id = ?")
            .bind(&owner)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    let local_path = local_path.expect("auto-created workspaces store a path");
    assert!(
        std::path::Path::new(&local_path).is_dir(),
        "expected a real directory at {local_path}"
    );
}
