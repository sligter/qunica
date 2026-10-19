use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
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

fn authenticated(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn claims(token: &str) -> Value {
    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap()
}

async fn register_account(app: &Router, email: &str) {
    let (status, _) = send(
        app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": email, "password": "supersecret", "name": "Device User"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

async fn login_account(app: &Router, email: &str) -> String {
    let (status, token) = send(
        app,
        post_json(
            "/api/v2/auth/login",
            json!({"email": email, "password": "supersecret", "device_label": "Test device"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    token["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn logout_and_owned_device_revocation_leave_other_sessions_active() {
    let (app, state) = qunica_backend::api::router_with_state_for_tests().await;
    register_account(&app, "devices@example.com").await;
    let first = login_account(&app, "devices@example.com").await;
    let second = login_account(&app, "devices@example.com").await;
    assert_ne!(first, second);
    let second_id = claims(&second)["session_id"].as_str().unwrap().to_string();
    let (status, sessions) =
        send(&app, authenticated("GET", "/api/v2/auth/sessions", &first)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    assert_eq!(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["current"] == true)
            .count(),
        1
    );
    assert!(!sessions.to_string().contains("credential_id"));
    assert!(!sessions.to_string().contains("access_token"));

    register_account(&app, "other@example.com").await;
    let other = login_account(&app, "other@example.com").await;
    let path = format!("/api/v2/auth/sessions/{second_id}");
    assert_eq!(
        send(&app, authenticated("DELETE", &path, &other)).await.0,
        StatusCode::NOT_FOUND
    );
    for _ in 0..2 {
        assert_eq!(
            send(&app, authenticated("DELETE", &path, &first)).await.0,
            StatusCode::NO_CONTENT
        );
    }
    for path in [
        "/api/v2/auth/me",
        "/api/v2/groups",
        "/api/v2/agents",
        "/api/v2/auth/sessions",
    ] {
        let (status, error) = send(&app, authenticated("GET", path, &second)).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "revoked session accepted at {path}"
        );
        assert_eq!(error["error"]["code"], "session_revoked");
    }
    let restarted = qunica_backend::api::router(state);
    assert_eq!(
        send(&restarted, authenticated("GET", "/api/v2/auth/me", &first))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&restarted, authenticated("GET", "/api/v2/auth/me", &second))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &restarted,
            authenticated("POST", "/api/v2/auth/logout", &first)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&restarted, authenticated("GET", "/api/v2/auth/me", &first))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&restarted, authenticated("GET", "/api/v2/auth/me", &other))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn paired_device_session_has_an_independent_credential_and_revocation() {
    let (app, state) = qunica_backend::api::router_with_state_for_tests().await;
    register_account(&app, "pairing@example.com").await;
    let desktop = login_account(&app, "pairing@example.com").await;
    let mut request = post_json("/api/v2/auth/sessions", json!({"device_label": "My phone"}));
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {desktop}").parse().unwrap(),
    );
    let (status, result) = send(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    let phone = result["access_token"].as_str().unwrap();
    assert_ne!(claims(phone)["session_id"], claims(&desktop)["session_id"]);
    assert_ne!(claims(phone)["jti"], claims(&desktop)["jti"]);
    assert!(claims(phone).get("exp").is_none());
    assert_eq!(
        send(&app, authenticated("POST", "/api/v2/auth/logout", phone))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, authenticated("GET", "/api/v2/auth/me", &desktop))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, authenticated("GET", "/api/v2/auth/me", phone))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    // Signature alone is insufficient: both account and credential must match
    // the live session row, including when two instances share a signing key.
    let original = claims(&desktop);
    for changed in [
        json!({"sub": "different-owner", "session_id": original["session_id"], "jti": original["jti"]}),
        json!({"sub": original["sub"], "session_id": original["session_id"], "jti": "wrong-credential"}),
        json!({"sub": original["sub"], "session_id": "unknown-session", "jti": original["jti"]}),
        json!({"sub": original["sub"], "session_id": original["session_id"]}),
    ] {
        let forged = encode(
            &Header::new(Algorithm::HS256),
            &changed,
            &EncodingKey::from_secret(state.auth.secret_key.as_bytes()),
        )
        .unwrap();
        assert_eq!(
            send(&app, authenticated("GET", "/api/v2/auth/me", &forged))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn revocation_ends_an_idle_sse_without_ending_another_device_stream() {
    use futures_util::{stream, StreamExt};
    use qunica_backend::api::auth::guard_session_stream;
    let (app, state) = qunica_backend::api::router_with_state_for_tests().await;
    register_account(&app, "streams@example.com").await;
    let first = login_account(&app, "streams@example.com").await;
    let second = login_account(&app, "streams@example.com").await;
    let stream_app = Router::new()
        .route(
            "/events",
            axum::routing::get(|| async {
                let frames = stream::once(async { Ok::<_, std::io::Error>("data: ready\n\n") })
                    .chain(stream::pending());
                (
                    [("content-type", "text/event-stream")],
                    Body::from_stream(frames),
                )
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state,
            guard_session_stream,
        ));
    let mut first_stream = stream_app
        .clone()
        .oneshot(authenticated("GET", "/events", &first))
        .await
        .unwrap()
        .into_body()
        .into_data_stream();
    let mut second_stream = stream_app
        .oneshot(authenticated("GET", "/events", &second))
        .await
        .unwrap()
        .into_body()
        .into_data_stream();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), first_stream.next())
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), second_stream.next())
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            send(&app, authenticated("POST", "/api/v2/auth/logout", &first))
        )
        .await
        .unwrap()
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), first_stream.next())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), second_stream.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sessions_and_revocations_survive_database_reopen_and_repeated_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite:{}?mode=rwc",
        dir.path().join("auth.sqlite3").display()
    );
    let (_, mut state) = qunica_backend::api::router_with_state_for_tests().await;
    state.db = Db::connect(&url).await.unwrap();
    state.db.migrate().await.unwrap();
    state.db.migrate().await.unwrap();
    let app = qunica_backend::api::router(state.clone());
    register_account(&app, "restart@example.com").await;
    let active = login_account(&app, "restart@example.com").await;
    let revoked = login_account(&app, "restart@example.com").await;
    assert_eq!(
        send(&app, authenticated("POST", "/api/v2/auth/logout", &revoked))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let credentials: Vec<String> = sqlx::query_scalar("SELECT credential_id FROM auth_sessions")
        .fetch_all(state.db.pool())
        .await
        .unwrap();
    assert!(credentials
        .iter()
        .all(|value| !value.contains('.') && value != &active && value != &revoked));
    drop(app);
    state.db.pool().close().await;
    state.db = Db::connect(&url).await.unwrap();
    state.db.migrate().await.unwrap();
    let app = qunica_backend::api::router(state);
    assert_eq!(
        send(&app, authenticated("GET", "/api/v2/auth/me", &active))
            .await
            .0,
        StatusCode::OK
    );
    let (status, error) = send(&app, authenticated("GET", "/api/v2/auth/me", &revoked)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"]["code"], "session_revoked");
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
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(access_token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["sub"], user_id);
    assert!(claims["session_id"].as_str().is_some());
    assert!(claims["jti"].as_str().is_some());
    assert!(claims.get("exp").is_none(), "login tokens must not expire");

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
async fn legacy_login_requires_reauthentication_without_creating_a_session() {
    let (app, state) = qunica_backend::api::router_with_state_for_tests().await;
    let (status, user) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "persistent@example.com", "password": "supersecret", "name": "Persistent"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    // Both legacy formats require a fresh interactive login, regardless of exp.
    for claims in [
        json!({"sub": user["id"], "exp": 1}),
        json!({"sub": user["id"]}),
    ] {
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(state.auth.secret_key.as_bytes()),
        )
        .unwrap();
        let restarted = qunica_backend::api::router(state.clone());
        let (status, error) = send(
            &restarted,
            Request::builder()
                .uri("/api/v2/auth/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error["error"]["code"], "session_upgrade_required");
    }
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}

#[tokio::test]
async fn persistent_login_still_rejects_invalid_credentials() {
    let (app, state) = qunica_backend::api::router_with_state_for_tests().await;
    let (status, user) = send(
        &app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": "signature@example.com", "password": "supersecret", "name": "Signature"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    for (claims, secret, algorithm) in [
        (
            json!({"sub": "unknown-user"}),
            state.auth.secret_key.as_str(),
            Algorithm::HS256,
        ),
        (json!({"sub": user["id"]}), "wrong-secret", Algorithm::HS256),
        (json!({}), state.auth.secret_key.as_str(), Algorithm::HS256),
        (
            json!({"sub": user["id"]}),
            state.auth.secret_key.as_str(),
            Algorithm::HS384,
        ),
    ] {
        let token = encode(
            &Header::new(algorithm),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        let (status, _) = send(
            &app,
            Request::builder()
                .uri("/api/v2/auth/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
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
