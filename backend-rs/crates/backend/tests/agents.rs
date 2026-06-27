use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

// Hardcoded valid UUIDs used for UUID-shaped fields (skill ids, provider ids).
const SKILL_A: &str = "11111111-1111-1111-1111-111111111111";
const SKILL_B: &str = "22222222-2222-2222-2222-222222222222";
const PROVIDER_A: &str = "33333333-3333-3333-3333-333333333333";

async fn app() -> Router {
    ag_swarmer_backend::api::router_for_tests().await
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

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// Register and log in a user, returning a bearer token.
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

/// Create a cloud-sandbox workspace (no local path required) and return its id.
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

async fn create_provider(app: &Router, token: &str, name: &str) -> String {
    let (status, provider) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/llm-providers",
            token,
            json!({
                "name": name,
                "kind": "openai-compatible",
                "base_url": "https://llm.example.test/v1",
                "api_key": format!("secret-{name}-1234"),
                "default_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    provider["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn agent_create_requires_active_owned_workspace() {
    let app = app().await;
    let token_a = register_and_login(&app, "ownera@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({"name": "Helper", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(agent["workspace_id"], workspace_a);
    assert_eq!(agent["status"], "active");
    assert_eq!(agent["runtime_kind"], "llm_chat");
    assert_eq!(agent["system_prompt"], "You are a helpful AI agent.");

    // A workspace owned by another user cannot be referenced.
    let token_b = register_and_login(&app, "ownerb@example.com").await;
    let workspace_b = create_workspace(&app, &token_b).await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({"name": "Trespasser", "workspace_id": workspace_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn agent_create_validates_provider_owner_and_status() {
    let app = app().await;
    let token_a = register_and_login(&app, "provider-create-a@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let provider_a = create_provider(&app, &token_a, "Owner A").await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Bound",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_a
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(agent["llm_provider_id"], provider_a);

    let token_b = register_and_login(&app, "provider-create-b@example.com").await;
    let provider_b = create_provider(&app, &token_b, "Owner B").await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Trespasser",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_b
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/llm-providers/{provider_a}"),
            &token_a,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Deleted Provider",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_a
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn agent_list_is_owner_scoped() {
    let app = app().await;
    let token_a = register_and_login(&app, "lista@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({"name": "A's Agent", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let token_b = register_and_login(&app, "listb@example.com").await;
    let (status, list_b) = send(&app, authed("GET", "/api/v2/agents", &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let b_ids: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(!b_ids.contains(&agent_id.as_str()));
}

#[tokio::test]
async fn agent_patch_updates_name_and_json_fields() {
    let app = app().await;
    let token = register_and_login(&app, "patch@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token,
            json!({"name": "Before", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token,
            json!({
                "name": "After",
                "llm_config": {"model": "claude", "temperature": 0.5},
                "tool_config": {"enabled": ["search"]},
                "skill_ids": [SKILL_A, SKILL_B],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "After");
    assert_eq!(
        updated["llm_config"],
        json!({"model": "claude", "temperature": 0.5})
    );
    assert_eq!(updated["tool_config"], json!({"enabled": ["search"]}));
    assert_eq!(updated["skill_ids"], json!([SKILL_A, SKILL_B]));

    // Values round-trip through a fresh GET.
    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["name"], "After");
    assert_eq!(
        fetched["llm_config"],
        json!({"model": "claude", "temperature": 0.5})
    );
    assert_eq!(fetched["tool_config"], json!({"enabled": ["search"]}));
    assert_eq!(fetched["skill_ids"], json!([SKILL_A, SKILL_B]));
}

#[tokio::test]
async fn agent_update_validates_provider_owner_and_status() {
    let app = app().await;
    let token_a = register_and_login(&app, "provider-update-a@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let provider_a = create_provider(&app, &token_a, "Original").await;
    let provider_a_next = create_provider(&app, &token_a, "Replacement").await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Switchable",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_a
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token_a,
            json!({"llm_provider_id": provider_a_next}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["llm_provider_id"], provider_a_next);

    let token_b = register_and_login(&app, "provider-update-b@example.com").await;
    let provider_b = create_provider(&app, &token_b, "Foreign").await;

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token_a,
            json!({"llm_provider_id": provider_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/llm-providers/{provider_a_next}"),
            &token_a,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token_a,
            json!({"llm_provider_id": provider_a_next}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn agent_runtime_kind_acp_clears_provider_and_stores_runtime() {
    let app = app().await;
    let token = register_and_login(&app, "acp@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    // Create an ACP agent with a provider and runtime: provider must be cleared.
    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token,
            json!({
                "name": "ACP Agent",
                "workspace_id": workspace,
                "runtime_kind": "acp",
                "llm_provider_id": PROVIDER_A,
                "acp_runtime": {"command": "claude-acp", "args": ["--flag"]},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();
    assert_eq!(agent["runtime_kind"], "acp");
    assert_eq!(agent["llm_provider_id"], Value::Null);
    assert_eq!(
        agent["acp_runtime"],
        json!({"command": "claude-acp", "args": ["--flag"]})
    );

    // Patching the same ACP agent with another provider must keep it cleared.
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token,
            json!({
                "llm_provider_id": PROVIDER_A,
                "acp_runtime": {"command": "claude-acp", "args": ["--v2"]},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["llm_provider_id"], Value::Null);
    assert_eq!(
        updated["acp_runtime"],
        json!({"command": "claude-acp", "args": ["--v2"]})
    );
}

#[tokio::test]
async fn agent_delete_soft_deletes_and_hides_from_list() {
    let app = app().await;
    let token = register_and_login(&app, "delete@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token,
            json!({"name": "Doomed", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    // Get now returns 404.
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    // List omits it.
    let (status, list) = send(&app, authed("GET", "/api/v2/agents", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&agent_id.as_str()));
}
