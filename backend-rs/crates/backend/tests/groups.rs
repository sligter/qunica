use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

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

#[tokio::test]
async fn group_create_requires_active_owned_workspace() {
    let app = app().await;
    let token_a = register_and_login(&app, "ownera@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token_a,
            json!({"name": "Team", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(group["workspace_id"], workspace_a);
    assert_eq!(group["status"], "active");
    // Defaults are exposed as booleans/numbers.
    assert_eq!(group["free_speech"], false);
    assert_eq!(group["proactive_mode"], false);
    assert_eq!(group["proactive_reply_multiplier"], 1);
    assert_eq!(group["allow_agent_free_mention"], true);

    // A workspace owned by another user cannot be referenced.
    let token_b = register_and_login(&app, "ownerb@example.com").await;
    let workspace_b = create_workspace(&app, &token_b).await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token_a,
            json!({"name": "Trespasser", "workspace_id": workspace_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn group_list_is_owner_scoped() {
    let app = app().await;
    let token_a = register_and_login(&app, "lista@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token_a,
            json!({"name": "A's Group", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let token_b = register_and_login(&app, "listb@example.com").await;
    let (status, list_b) = send(&app, authed("GET", "/api/v2/groups", &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let b_ids: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert!(!b_ids.contains(&group_id.as_str()));
}

#[tokio::test]
async fn group_patch_updates_name_description_workspace_and_settings() {
    let app = app().await;
    let token = register_and_login(&app, "patch@example.com").await;
    let workspace_a = create_workspace(&app, &token).await;
    let workspace_b = create_workspace(&app, &token).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Before", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({
                "name": "After",
                "description": "the team chat",
                "workspace_id": workspace_b,
                "free_speech": true,
                "proactive_mode": true,
                "proactive_reply_multiplier": 3,
                "allow_agent_free_mention": false,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "After");
    assert_eq!(updated["description"], "the team chat");
    assert_eq!(updated["workspace_id"], workspace_b);
    assert_eq!(updated["free_speech"], true);
    assert_eq!(updated["proactive_mode"], true);
    assert_eq!(updated["proactive_reply_multiplier"], 3);
    assert_eq!(updated["allow_agent_free_mention"], false);

    // Values round-trip through a fresh GET.
    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["name"], "After");
    assert_eq!(fetched["description"], "the team chat");
    assert_eq!(fetched["workspace_id"], workspace_b);
    assert_eq!(fetched["free_speech"], true);
    assert_eq!(fetched["proactive_mode"], true);
    assert_eq!(fetched["proactive_reply_multiplier"], 3);
    assert_eq!(fetched["allow_agent_free_mention"], false);

    // Explicit null clears the workspace binding.
    let (status, cleared) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({"workspace_id": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["workspace_id"], Value::Null);
}

#[tokio::test]
async fn group_rejects_invalid_reply_multiplier() {
    let app = app().await;
    let token = register_and_login(&app, "multiplier@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    // Create with multiplier 0 is rejected.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Bad", "workspace_id": workspace, "proactive_reply_multiplier": 0}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    // A valid group can still be created, then PATCH with 0 is rejected.
    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Good", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({"proactive_reply_multiplier": 0}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_delete_soft_deletes_and_hides_from_list() {
    let app = app().await;
    let token = register_and_login(&app, "delete@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Doomed", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    // Get now returns 404.
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    // List omits it.
    let (status, list) = send(&app, authed("GET", "/api/v2/groups", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&group_id.as_str()));
}
