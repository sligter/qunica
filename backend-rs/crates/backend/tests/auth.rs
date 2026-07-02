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

#[tokio::test]
async fn register_login_and_me_round_trip() {
    let app = app().await;

    let (status, user) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "Alice@Example.com", "password": "supersecret", "name": "Alice"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(user["email"], "alice@example.com");
    assert_eq!(user["name"], "Alice");
    let user_id = user["id"].as_str().unwrap().to_string();
    assert!(!user_id.is_empty());

    let (status, token) = send(
        &app,
        post_json(
            "/api/v2/auth/login",
            json!({"email": "alice@example.com", "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(token["token_type"], "bearer");
    let access_token = token["access_token"].as_str().unwrap().to_string();
    assert!(!access_token.is_empty());

    let me_request = Request::builder()
        .method("GET")
        .uri("/api/v2/auth/me")
        .header("authorization", format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    let (status, me) = send(&app, me_request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["id"], user_id);
    assert_eq!(me["email"], "alice@example.com");
    assert_eq!(me["name"], "Alice");
}

#[tokio::test]
async fn register_rejects_duplicate_email() {
    let app = app().await;

    let (status, _) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "dup@example.com", "password": "supersecret", "name": "First"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "DUP@Example.com", "password": "anothersecret", "name": "Second"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let app = app().await;

    let (status, _) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "bob@example.com", "password": "correcthorse", "name": "Bob"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        post_json(
            "/api/v2/auth/login",
            json!({"email": "bob@example.com", "password": "wrongpassword"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
    assert_eq!(body["error"]["message"], "invalid credentials");
}

#[tokio::test]
async fn me_requires_bearer_token() {
    let app = app().await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/v2/auth/me")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
}
