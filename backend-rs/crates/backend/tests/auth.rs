use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

use qunica_backend::{config::InitialUserConfig, db::Db};

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

    let update_request = Request::builder()
        .method("PATCH")
        .uri("/api/v2/auth/me")
        .header("authorization", format!("Bearer {access_token}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"name": " Alice Nova ", "avatar_url": "preset:prism"}).to_string(),
        ))
        .unwrap();
    let (status, updated) = send(&app, update_request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "Alice Nova");
    assert_eq!(updated["avatar_url"], "preset:prism");

    let me_request = Request::builder()
        .method("GET")
        .uri("/api/v2/auth/me")
        .header("authorization", format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    let (status, me) = send(&app, me_request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["name"], "Alice Nova");
    assert_eq!(me["avatar_url"], "preset:prism");

    let clear_request = Request::builder()
        .method("PATCH")
        .uri("/api/v2/auth/me")
        .header("authorization", format!("Bearer {access_token}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"avatar_url": null}).to_string()))
        .unwrap();
    let (status, cleared) = send(&app, clear_request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["name"], "Alice Nova");
    assert!(cleared["avatar_url"].is_null());

    let invalid_name_request = Request::builder()
        .method("PATCH")
        .uri("/api/v2/auth/me")
        .header("authorization", format!("Bearer {access_token}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"name": "   "}).to_string()))
        .unwrap();
    let (status, body) = send(&app, invalid_name_request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
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

#[tokio::test]
async fn disabled_registration_is_publicly_reported_and_enforced() {
    let (_, mut state) = qunica_backend::api::router_with_state_for_tests().await;
    state.auth.registration_enabled = false;
    let app = qunica_backend::api::router(state);

    let config_request = Request::builder()
        .uri("/api/v2/auth/config")
        .body(Body::empty())
        .unwrap();
    let (status, config) = send(&app, config_request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["registration_enabled"], false);

    let (status, body) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "blocked@example.com", "password": "supersecret", "name": "Blocked"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "registration_disabled");
}

#[tokio::test]
async fn initial_user_is_created_only_for_an_empty_database() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let initial = InitialUserConfig {
        email: "Admin@Example.com".into(),
        password: "initial-password".into(),
        name: "Administrator".into(),
    };
    qunica_backend::api::auth::initialize_auth(db.pool(), false, Some(&initial))
        .await
        .unwrap();

    let (email, name, password_hash): (String, String, String) =
        sqlx::query_as("SELECT email, name, password_hash FROM users")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(email, "admin@example.com");
    assert_eq!(name, "Administrator");
    assert!(bcrypt::verify("initial-password", &password_hash).unwrap());

    let replacement = InitialUserConfig {
        email: "second@example.com".into(),
        password: "replacement-password".into(),
        name: "Second".into(),
    };
    qunica_backend::api::auth::initialize_auth(db.pool(), false, Some(&replacement))
        .await
        .unwrap();
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(user_count, 1);
}

#[tokio::test]
async fn empty_database_cannot_start_locked_without_an_initial_user() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let error = qunica_backend::api::auth::initialize_auth(db.pool(), false, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("database has no users"));
}
