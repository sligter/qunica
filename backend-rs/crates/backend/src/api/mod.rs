pub mod agents;
pub mod auth;
pub mod error;
pub mod groups;
pub mod health;
pub mod messages;
pub mod workspaces;

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use tokio::sync::Mutex;

use crate::db::Db;

/// Shared application state injected into every API v2 handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub auth: AuthSettings,
    /// Serializes chat/runtime writes so per-thread sequence allocation stays
    /// atomic across concurrent streams on the same SQLite database.
    pub write_lock: Arc<Mutex<()>>,
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
        .route(
            "/api/v2/workspaces",
            post(workspaces::create).get(workspaces::list),
        )
        .route(
            "/api/v2/workspaces/:workspace_id",
            get(workspaces::get)
                .patch(workspaces::update)
                .delete(workspaces::delete),
        )
        .route(
            "/api/v2/agents",
            post(agents::create).get(agents::list),
        )
        .route(
            "/api/v2/agents/:agent_id",
            get(agents::get)
                .patch(agents::update)
                .delete(agents::delete),
        )
        .route(
            "/api/v2/groups",
            post(groups::create).get(groups::list),
        )
        .route(
            "/api/v2/groups/:group_id",
            get(groups::get)
                .patch(groups::update)
                .delete(groups::delete),
        )
        .route(
            "/api/v2/groups/:group_id/messages/stream",
            post(messages::stream),
        )
        .with_state(state)
}

/// Build a router backed by a fresh, migrated in-memory database for tests.
#[doc(hidden)]
pub async fn router_for_tests() -> Router {
    router_with_state_for_tests().await.0
}

/// Like [`router_for_tests`], but also returns the [`AppState`] so tests can
/// seed rows directly through the shared pool (there is no group-agent or
/// provider binding API yet).
#[doc(hidden)]
pub async fn router_with_state_for_tests() -> (Router, AppState) {
    let db = Db::connect("sqlite::memory:")
        .await
        .expect("connect test db");
    db.migrate().await.expect("migrate test db");
    let state = AppState {
        db,
        auth: AuthSettings {
            secret_key: "test-secret".to_string(),
            access_token_expire_minutes: 10080,
        },
        write_lock: Arc::new(Mutex::new(())),
    };
    (router(state.clone()), state)
}
