//! Built-in Assistant agent bootstrap and visibility tests.
//!
//! The Assistant is an ordinary `llm_chat` agent row flagged `is_system = 1`
//! with no bound workspace, reached through an ordinary direct chat. These
//! tests pin the two properties that make that safe: it is created lazily and
//! exactly once per owner, and it is invisible to — and unwritable through —
//! the generic agent and direct-chat routes.

use ag_swarmer_backend::api::router_for_tests;
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

async fn get_assistant(app: &Router, token: &str) -> Value {
    let (status, body) = send(app, authed("GET", "/api/v2/assistant", token)).await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    body
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
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_str().unwrap().to_string()
}

async fn create_provider(app: &Router, token: &str) -> String {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/llm-providers",
            Some(token),
            json!({
                "name": "Primary",
                "kind": "openai-compatible",
                "base_url": "https://example.invalid/v1",
                "api_key": "sk-not-a-real-key",
                "default_model": "gpt-4o-mini"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn assistant_is_created_lazily_and_reused() {
    let app = router_for_tests().await;
    let token = register(&app, "assistant-bootstrap@example.com").await;

    let first = get_assistant(&app, &token).await;
    let second = get_assistant(&app, &token).await;

    assert_eq!(first["agent_id"], second["agent_id"]);
    assert_eq!(first["chat_id"], second["chat_id"]);
    assert!(first["agent_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(first["chat_id"].as_str().is_some_and(|id| !id.is_empty()));
    // A fresh account has no provider, so the dock must fall back to its
    // scripted checklist rather than trying to talk.
    assert_eq!(first["provider_configured"], json!(false));
}

#[tokio::test]
async fn assistant_has_no_workspace_so_file_tools_stay_unreachable() {
    let app = router_for_tests().await;
    let token = register(&app, "assistant-no-workspace@example.com").await;

    let assistant = get_assistant(&app, &token).await;
    let chat_id = assistant["chat_id"].as_str().unwrap();

    let (status, chat) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {chat:?}");
    assert_eq!(chat["workspace_id"], Value::Null);
}

#[tokio::test]
async fn assistant_is_hidden_from_the_agent_and_chat_lists() {
    let app = router_for_tests().await;
    let token = register(&app, "assistant-hidden@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    // A normal agent and its chat stay visible; only the Assistant is filtered.
    let (status, normal) = send(
        &app,
        request(
            "POST",
            "/api/v2/agents",
            Some(&token),
            json!({"name": "Researcher", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {normal:?}");
    let normal_agent = normal["id"].as_str().unwrap().to_string();
    let (status, normal_chat) = send(
        &app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(&token),
            json!({"agent_id": normal_agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {normal_chat:?}");
    let normal_chat_id = normal_chat["id"].as_str().unwrap().to_string();

    let assistant = get_assistant(&app, &token).await;
    let assistant_agent = assistant["agent_id"].as_str().unwrap();
    let assistant_chat = assistant["chat_id"].as_str().unwrap();

    let (status, agents) = send(&app, authed("GET", "/api/v2/agents", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let agents = agents.as_array().unwrap();
    assert!(agents.iter().all(|agent| agent["id"] != assistant_agent));
    assert!(agents
        .iter()
        .any(|agent| agent["id"] == normal_agent.as_str()));

    let (status, chats) = send(&app, authed("GET", "/api/v2/direct-chats", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let chats = chats.as_array().unwrap();
    assert!(chats.iter().all(|chat| chat["id"] != assistant_chat));
    assert!(chats
        .iter()
        .any(|chat| chat["id"] == normal_chat_id.as_str()));
}

#[tokio::test]
async fn assistant_cannot_be_edited_or_deleted_through_the_agent_routes() {
    let app = router_for_tests().await;
    let token = register(&app, "assistant-guarded@example.com").await;
    let assistant = get_assistant(&app, &token).await;
    let agent_id = assistant["agent_id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            Some(&token),
            json!({"name": "Hijacked"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");

    // Still reachable afterwards.
    let (status, _) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn assistant_provider_can_be_bound_and_is_reported() {
    let app = router_for_tests().await;
    let token = register(&app, "assistant-provider@example.com").await;
    let provider = create_provider(&app, &token).await;

    let before = get_assistant(&app, &token).await;
    assert_eq!(before["provider_configured"], json!(false));

    let (status, body) = send(
        &app,
        request(
            "PATCH",
            "/api/v2/assistant",
            Some(&token),
            json!({"llm_provider_id": provider}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["provider_configured"], json!(true));
    assert_eq!(body["provider_id"], json!(provider));

    let after = get_assistant(&app, &token).await;
    assert_eq!(after["provider_configured"], json!(true));
    assert_eq!(after["agent_id"], before["agent_id"]);
}

#[tokio::test]
async fn assistant_provider_must_belong_to_the_caller() {
    let app = router_for_tests().await;
    let owner = register(&app, "assistant-owner@example.com").await;
    let stranger = register(&app, "assistant-stranger@example.com").await;
    let stranger_provider = create_provider(&app, &stranger).await;

    get_assistant(&app, &owner).await;
    let (status, body) = send(
        &app,
        request(
            "PATCH",
            "/api/v2/assistant",
            Some(&owner),
            json!({"llm_provider_id": stranger_provider}),
        ),
    )
    .await;
    // Matches `agents::validate_provider`: another user's row is a permission
    // failure, not a malformed request.
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
}

#[tokio::test]
async fn assistant_provider_must_exist() {
    let app = router_for_tests().await;
    let token = register(&app, "assistant-missing-provider@example.com").await;
    get_assistant(&app, &token).await;

    let (status, body) = send(
        &app,
        request(
            "PATCH",
            "/api/v2/assistant",
            Some(&token),
            json!({"llm_provider_id": "00000000-0000-4000-8000-000000000000"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
}

#[tokio::test]
async fn each_owner_gets_their_own_assistant() {
    let app = router_for_tests().await;
    let first = register(&app, "assistant-first@example.com").await;
    let second = register(&app, "assistant-second@example.com").await;

    let one = get_assistant(&app, &first).await;
    let two = get_assistant(&app, &second).await;

    assert_ne!(one["agent_id"], two["agent_id"]);
    assert_ne!(one["chat_id"], two["chat_id"]);
}
