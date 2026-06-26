use std::path::Path;

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

#[tokio::test]
async fn workspace_create_normalizes_local_path_and_is_owner_scoped() {
    let app = app().await;
    let token = register_and_login(&app, "owner@example.com").await;

    let dir = tempfile::tempdir().unwrap();
    let raw_path = dir.path().to_str().unwrap().to_string();

    let (status, workspace) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "Local WS", "backend_type": "local", "local_path": raw_path}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(workspace["name"], "Local WS");
    assert_eq!(workspace["backend_type"], "local");
    assert_eq!(workspace["status"], "active");

    let stored = workspace["local_path"].as_str().unwrap();
    let expected = std::fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(stored, expected);
    assert!(Path::new(stored).is_absolute());

    let workspace_id = workspace["id"].as_str().unwrap().to_string();

    let (status, list) = send(&app, authed("GET", "/api/v2/workspaces", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&workspace_id.as_str()));
}

#[tokio::test]
async fn workspace_create_rejects_missing_or_nonexistent_local_path() {
    let app = app().await;
    let token = register_and_login(&app, "pathcheck@example.com").await;

    // Missing local_path for a local backend.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "No Path", "backend_type": "local"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    // Nonexistent local_path.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({
                "name": "Bad Path",
                "backend_type": "local",
                "local_path": "/this/path/does/not/exist/ag-swarmer-xyz"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn workspace_list_is_owner_scoped() {
    let app = app().await;
    let token_a = register_and_login(&app, "usera@example.com").await;
    let token_b = register_and_login(&app, "userb@example.com").await;

    let dir = tempfile::tempdir().unwrap();
    let raw_path = dir.path().to_str().unwrap().to_string();

    let (status, workspace) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token_a,
            json!({"name": "A WS", "backend_type": "local", "local_path": raw_path}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let a_id = workspace["id"].as_str().unwrap().to_string();

    let (status, list_b) = send(&app, authed("GET", "/api/v2/workspaces", &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let b_ids: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_str().unwrap())
        .collect();
    assert!(!b_ids.contains(&a_id.as_str()));
}

#[tokio::test]
async fn workspace_patch_renames_and_preserves_binding() {
    let app = app().await;
    let token = register_and_login(&app, "rename@example.com").await;

    let dir = tempfile::tempdir().unwrap();
    let raw_path = dir.path().to_str().unwrap().to_string();

    let (status, workspace) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "Before", "backend_type": "local", "local_path": raw_path}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace_id = workspace["id"].as_str().unwrap().to_string();
    let original_path = workspace["local_path"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/workspaces/{workspace_id}"),
            &token,
            json!({"name": "After"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "After");
    assert_eq!(updated["local_path"].as_str().unwrap(), original_path);
    assert_eq!(updated["backend_type"], "local");
}

#[tokio::test]
async fn workspace_delete_soft_deletes_and_hides_from_list() {
    let app = app().await;
    let token = register_and_login(&app, "delete@example.com").await;

    let dir = tempfile::tempdir().unwrap();
    let raw_path = dir.path().to_str().unwrap().to_string();

    let (status, workspace) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "Doomed", "backend_type": "local", "local_path": raw_path}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace_id = workspace["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/workspaces/{workspace_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    // Get still returns it, now marked deleted.
    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/workspaces/{workspace_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["status"], "deleted");

    // List omits it.
    let (status, list) = send(&app, authed("GET", "/api/v2/workspaces", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&workspace_id.as_str()));
}
