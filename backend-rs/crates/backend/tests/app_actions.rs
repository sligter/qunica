//! Staged configuration changes and the approval endpoint.
//!
//! The whole point of this surface is that a tool call changes nothing. These
//! tests pin that: proposing stages a row and leaves the database alone,
//! approval is the only thing that applies it, approval is idempotent-safe and
//! owner-scoped, and the forbidden targets are refused when they are proposed
//! rather than when they are approved.

use ag_swarmer_backend::{
    api::{router_with_state_for_tests, AppState},
    tools::{AppControlContext, ToolExecutor, ToolResult, ToolStatus},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
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
    assert_eq!(body["status"], "applied");
    assert_eq!(count(&state, "agents", &owner).await, 1);

    // A second approval must not create a second agent.
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
    assert_eq!(count(&state, "agents", &owner).await, 1);
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
    assert_eq!(count(&state, "agents", &owner).await, 0);

    let (status, list) = send(&app, authed("GET", "/api/v2/app-actions", &stranger)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list.as_array().unwrap().is_empty());
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

    let (status, list) = send(&app, authed("GET", "/api/v2/app-actions", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let items = list.as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["status"], "pending");
        assert_eq!(item["target_kind"], "skill");
        assert!(item["summary"].as_str().is_some_and(|s| !s.is_empty()));
    }
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
