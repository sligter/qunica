use std::path::Path;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn app() -> Router {
    qunica_backend::api::router_for_tests().await
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
async fn workspace_create_can_make_a_random_local_directory() {
    let app = app().await;
    let token = register_and_login(&app, "automatic@example.com").await;
    let root = tempfile::tempdir().unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"group_workspace_root": root.path()}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, workspace) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "Automatic", "backend_type": "local", "auto_create": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let path = Path::new(workspace["local_path"].as_str().unwrap());
    let canonical_root = std::fs::canonicalize(root.path()).unwrap();
    assert!(path.is_dir());
    assert_eq!(path.parent(), Some(canonical_root.as_path()));
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("workspace-"));
}

#[tokio::test]
async fn workspace_relative_paths_create_and_rebind_inside_the_configured_root() {
    let app = app().await;
    let token = register_and_login(&app, "relative@example.com").await;
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("workspaces");
    std::fs::create_dir(&root).unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"group_workspace_root": root}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, workspace) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "DSV4 Flash", "backend_type": "local", "local_path": "dsv4-flash"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let expected = std::fs::canonicalize(root.join("dsv4-flash")).unwrap();
    assert_eq!(workspace["local_path"], expected.to_string_lossy().as_ref());

    let workspace_id = workspace["id"].as_str().unwrap();
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/workspaces/{workspace_id}"),
            &token,
            json!({"local_path": "managed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected = std::fs::canonicalize(root.join("managed")).unwrap();
    assert_eq!(updated["local_path"], expected.to_string_lossy().as_ref());

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            &token,
            json!({"name": "Escape", "backend_type": "local", "local_path": "../escape"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!base.path().join("escape").exists());
}

/// A browser can only ever show the *client* machine's folders, so a remote
/// deployment has to enumerate directories server side.
#[tokio::test]
async fn workspace_directories_browse_inside_the_configured_root() {
    let app = app().await;
    let token = register_and_login(&app, "browse@example.com").await;
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("workspaces");
    std::fs::create_dir_all(root.join("alpha").join("nested")).unwrap();
    std::fs::create_dir(root.join("beta")).unwrap();
    std::fs::write(root.join("notes.md"), "not a directory").unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"group_workspace_root": root}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, listing) = send(
        &app,
        authed("GET", "/api/v2/workspaces/directories", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["relative_path"], "");
    assert_eq!(listing["parent_relative_path"], Value::Null);
    let names: Vec<&str> = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["alpha", "beta"], "files are not directories");

    let (status, nested) = send(
        &app,
        authed("GET", "/api/v2/workspaces/directories?path=alpha", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(nested["relative_path"], "alpha");
    assert_eq!(nested["parent_relative_path"], "");
    assert_eq!(nested["entries"][0]["name"], "nested");
    assert_eq!(nested["entries"][0]["relative_path"], "alpha/nested");
}

#[tokio::test]
async fn workspace_directories_create_child_inside_the_configured_root() {
    let app = app().await;
    let token = register_and_login(&app, "mkdir@example.com").await;
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("workspaces");
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    std::fs::create_dir(base.path().join("outside")).unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"group_workspace_root": root}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, directory) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces/directories",
            &token,
            json!({"parent": "alpha", "name": "new-project"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(directory["relative_path"], "alpha/new-project");
    assert!(root.join("alpha/new-project").is_dir());

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces/directories",
            &token,
            json!({"parent": "../outside", "name": "escape"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!base.path().join("outside/escape").exists());

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/workspaces/directories",
            &token,
            json!({"parent": "alpha", "name": "../escape"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!root.join("escape").exists());
}

#[tokio::test]
async fn workspace_directories_reject_paths_outside_the_root() {
    let app = app().await;
    let token = register_and_login(&app, "browse-escape@example.com").await;
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("workspaces");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(base.path().join("secrets")).unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"group_workspace_root": root}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for path in ["../secrets", ".."] {
        let (status, _) = send(
            &app,
            authed(
                "GET",
                &format!("/api/v2/workspaces/directories?path={path}"),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "escaping via {path}");
    }

    let absolute = base.path().join("secrets");
    let (status, _) = send(
        &app,
        authed(
            "GET",
            &format!(
                "/api/v2/workspaces/directories?path={}",
                urlencoded(&absolute.to_string_lossy())
            ),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn workspace_directories_require_authentication() {
    let app = app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/workspaces/directories")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Percent-encode the few characters a Windows path contributes to a query
/// string. Enough for a temp-dir path; not a general encoder.
fn urlencoded(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\\' => "%5C".to_string(),
            ':' => "%3A".to_string(),
            ' ' => "%20".to_string(),
            other => other.to_string(),
        })
        .collect()
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
                "local_path": "/this/path/does/not/exist/qunica-xyz"
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
        authed(
            "DELETE",
            &format!("/api/v2/workspaces/{workspace_id}"),
            &token,
        ),
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
