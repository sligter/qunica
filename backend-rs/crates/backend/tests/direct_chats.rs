use ag_swarmer_backend::api::{router_with_state_for_tests, AppState};
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

async fn create_workspace(app: &Router, token: &str) -> String {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/workspaces",
            Some(token),
            json!({"name":"Workspace", "backend_type":"cloud_sandbox"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_str().unwrap().to_string()
}

async fn create_agent(app: &Router, token: &str, workspace_id: &str, name: &str) -> String {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/agents",
            Some(token),
            json!({"name":name, "workspace_id":workspace_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_str().unwrap().to_string()
}

async fn create_chat(app: &Router, token: &str, agent_id: &str) -> Value {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(token),
            json!({"agent_id": agent_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    body
}

#[tokio::test]
async fn direct_chat_lifecycle_creates_independent_sessions_and_graph() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-lifecycle@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Atlas").await;
    let first = create_chat(&app, &token, &agent_id).await;
    let second = create_chat(&app, &token, &agent_id).await;
    assert_ne!(first["id"], second["id"]);
    assert_eq!(first["title"], "New chat with Atlas");
    assert_eq!(first["title_source"], "automatic");
    assert_eq!(first["workspace_id"], workspace_id);

    let first_id = first["id"].as_str().unwrap();
    let graph: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM group_members WHERE group_id = ? AND status = 'active'), \
                (SELECT COUNT(*) FROM group_agents WHERE group_id = ? AND agent_id = ? AND status = 'active'), \
                (SELECT COUNT(*) FROM threads WHERE group_id = ? AND status = 'active')",
    ).bind(first_id).bind(first_id).bind(&agent_id).bind(first_id).fetch_one(state.db.pool()).await.unwrap();
    assert_eq!(graph, (1, 1, 1));

    let (status, list) = send(&app, authed("GET", "/api/v2/direct-chats", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);

    let (status, renamed) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{first_id}"),
            Some(&token),
            json!({"title":"  Manual title  "}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["title"], "Manual title");
    assert_eq!(renamed["title_source"], "manual");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/direct-chats/{first_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{first_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn direct_chat_lifecycle_enforces_agent_state_ownership_and_kind() {
    let (app, state) = router_with_state_for_tests().await;
    let token_a = register(&app, "direct-owner@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let agent_a = create_agent(&app, &token_a, &workspace_a, "Owned").await;
    let chat = create_chat(&app, &token_a, &agent_a).await;
    let chat_id = chat["id"].as_str().unwrap();
    let token_b = register(&app, "direct-other@example.com").await;

    for call in [
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token_b),
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{chat_id}"),
            Some(&token_b),
            json!({"title":"No"}),
        ),
        authed(
            "DELETE",
            &format!("/api/v2/direct-chats/{chat_id}"),
            &token_b,
        ),
    ] {
        let (status, _) = send(&app, call).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let other_workspace = create_workspace(&app, &token_b).await;
    let other_agent = create_agent(&app, &token_b, &other_workspace, "Foreign").await;
    let (status, body) = send(
        &app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(&token_a),
            json!({"agent_id":other_agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    sqlx::query("UPDATE agents SET status = 'deleted' WHERE id = ?")
        .bind(&agent_a)
        .execute(state.db.pool())
        .await
        .unwrap();
    let (status, existing) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(existing["agent_status"], "deleted");
    let (status, _) = send(
        &app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(&token_a),
            json!({"agent_id":agent_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, group) = send(
        &app,
        request(
            "POST",
            "/api/v2/groups",
            Some(&token_a),
            json!({"name":"Normal group", "workspace_id":workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{group_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn direct_chat_titles_validate_and_follow_account_language() {
    let (app, _state): (Router, AppState) = router_with_state_for_tests().await;
    let token = register(&app, "direct-language@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "中文 Agent").await;
    let (status, _) = send(
        &app,
        request(
            "PATCH",
            "/api/v2/settings/system",
            Some(&token),
            json!({"language":"zh-CN"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let chat = create_chat(&app, &token, &agent_id).await;
    assert_eq!(chat["title"], "与 中文 Agent 的新对话");
    let chat_id = chat["id"].as_str().unwrap();
    for title in ["", " ", &"x".repeat(121)] {
        let (status, body) = send(
            &app,
            request(
                "PATCH",
                &format!("/api/v2/direct-chats/{chat_id}"),
                Some(&token),
                json!({"title": title}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }
}
