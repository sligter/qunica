//! The browser build has no native PTY, so these routes are the only terminal
//! a Docker deployment has. They must stay authenticated and workspace-scoped.

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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
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

async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v2/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": email, "name": "Terminal", "password": "terminal-pass-1234"})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v2/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": email, "password": "terminal-pass-1234"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["access_token"].as_str().unwrap().to_string()
}

/// Point the account's workspace root at a fresh temp directory.
async fn use_workspace_root(app: &Router, token: &str, root: &std::path::Path) {
    let (status, _) = send(
        app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            token,
            json!({"group_workspace_root": root}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn terminal_routes_require_authentication() {
    let app = app().await;
    // Bodies are well formed on purpose: axum runs the `Json` extractor before
    // the handler reaches its auth check, so a malformed body would answer 422
    // and prove nothing about authentication.
    let cases = [
        (
            "POST",
            "/api/v2/terminal/sessions",
            json!({"conversation_id": "c", "cwd": "/tmp", "cols": 80, "rows": 24}),
        ),
        ("DELETE", "/api/v2/terminal/sessions", Value::Null),
        ("GET", "/api/v2/terminal/sessions/abc/events", Value::Null),
        (
            "POST",
            "/api/v2/terminal/sessions/abc/input",
            json!({"data": "x"}),
        ),
        (
            "POST",
            "/api/v2/terminal/sessions/abc/resize",
            json!({"cols": 80, "rows": 24}),
        ),
        ("DELETE", "/api/v2/terminal/sessions/abc", Value::Null),
    ];
    for (method, uri, body) in cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} must be authenticated"
        );
    }
}

#[tokio::test]
async fn terminal_create_starts_a_session_inside_the_workspace_root() {
    let app = app().await;
    let token = register_and_login(&app, "terminal-create@example.com").await;
    let root = tempfile::tempdir().unwrap();
    use_workspace_root(&app, &token, root.path()).await;

    let (status, descriptor) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/terminal/sessions",
            &token,
            json!({
                "conversation_id": "conversation-1",
                "cwd": root.path(),
                "cols": 80,
                "rows": 24,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{descriptor}");
    let session_id = descriptor["session_id"].as_str().unwrap().to_string();
    assert!(!descriptor["shell_name"].as_str().unwrap().is_empty());

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/terminal/sessions/{session_id}/resize"),
            &token,
            json!({"cols": 120, "rows": 40}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(
        &app,
        authed_json(
            "DELETE",
            &format!("/api/v2/terminal/sessions/{session_id}"),
            &token,
            Value::Null,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The session is gone, so writing to it no longer resolves.
    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/terminal/sessions/{session_id}/input"),
            &token,
            json!({"data": "echo hi\n"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn terminal_create_rejects_a_cwd_outside_the_account_workspaces() {
    let app = app().await;
    let token = register_and_login(&app, "terminal-escape@example.com").await;
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("workspaces");
    std::fs::create_dir(&root).unwrap();
    let outside = base.path().join("elsewhere");
    std::fs::create_dir(&outside).unwrap();
    use_workspace_root(&app, &token, &root).await;

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/terminal/sessions",
            &token,
            json!({
                "conversation_id": "conversation-1",
                "cwd": outside,
                "cols": 80,
                "rows": 24,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/terminal/sessions",
            &token,
            json!({
                "conversation_id": "conversation-1",
                "cwd": root.join("missing"),
                "cols": 80,
                "rows": 24,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn terminal_sessions_are_invisible_to_another_account() {
    let app = app().await;
    let owner = register_and_login(&app, "terminal-owner@example.com").await;
    let intruder = register_and_login(&app, "terminal-intruder@example.com").await;
    let root = tempfile::tempdir().unwrap();
    use_workspace_root(&app, &owner, root.path()).await;

    let (status, descriptor) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/terminal/sessions",
            &owner,
            json!({
                "conversation_id": "conversation-1",
                "cwd": root.path(),
                "cols": 80,
                "rows": 24,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id = descriptor["session_id"].as_str().unwrap().to_string();

    for (method, suffix, body) in [
        ("GET", "/events", Value::Null),
        ("POST", "/input", json!({"data": "whoami\n"})),
        ("POST", "/resize", json!({"cols": 10, "rows": 10})),
    ] {
        let (status, _) = send(
            &app,
            authed_json(
                method,
                &format!("/api/v2/terminal/sessions/{session_id}{suffix}"),
                &intruder,
                body,
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "another account reached {suffix}"
        );
    }

    let (status, closed) = send(
        &app,
        authed_json("DELETE", "/api/v2/terminal/sessions", &owner, Value::Null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["closed"], 1);
}

/// The prompt is written before a client can open the event stream, so the
/// session has to buffer it. Without the replay the first screen is blank.
#[tokio::test]
async fn terminal_events_replay_output_written_before_the_stream_opened() {
    let app = app().await;
    let token = register_and_login(&app, "terminal-replay@example.com").await;
    let root = tempfile::tempdir().unwrap();
    use_workspace_root(&app, &token, root.path()).await;

    let (status, descriptor) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/terminal/sessions",
            &token,
            json!({
                "conversation_id": "conversation-1",
                "cwd": root.path(),
                "cols": 80,
                "rows": 24,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id = descriptor["session_id"].as_str().unwrap().to_string();

    // Give the shell time to print a prompt before anyone subscribes.
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v2/terminal/sessions/{session_id}/events"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("text/event-stream")),
        Some(true)
    );

    // The stream stays open for the session's lifetime, so only the buffered
    // head is read here.
    let mut body = response.into_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), {
        use futures_util::StreamExt;
        body.next()
    })
    .await
    .expect("an SSE frame within five seconds")
    .expect("a stream chunk")
    .expect("a readable chunk");
    let text = String::from_utf8_lossy(&first);
    assert!(
        text.contains("\"event\":\"output\"") || text.contains("keep-alive"),
        "unexpected first frame: {text}"
    );

    let (status, _) = send(
        &app,
        authed_json("DELETE", "/api/v2/terminal/sessions", &token, Value::Null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
