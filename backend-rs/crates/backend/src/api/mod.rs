pub mod auth;
pub mod error;
pub mod health;

use axum::{
    routing::{get, post},
    Router,
};

use crate::db::Db;

/// Shared application state injected into every API v2 handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub auth: AuthSettings,
}

/// Auth configuration needed to mint and verify access tokens.
#[derive(Clone)]
pub struct AuthSettings {
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health::health))
        .route("/api/v2/health", get(health::health))
        .route("/api/v2/auth/register", post(auth::register))
        .route("/api/v2/auth/login", post(auth::login))
        .route("/api/v2/auth/me", get(auth::me))
        .with_state(state)
}

/// Build a router backed by a fresh, migrated in-memory database for tests.
#[doc(hidden)]
pub async fn router_for_tests() -> Router {
    let db = Db::connect("sqlite::memory:")
        .await
        .expect("connect test db");
    db.migrate().await.expect("migrate test db");
    router(AppState {
        db,
        auth: AuthSettings {
            secret_key: "test-secret".to_string(),
            access_token_expire_minutes: 10080,
        },
    })
}
