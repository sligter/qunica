pub mod agents;
pub mod auth;
pub mod error;
pub mod groups;
pub mod health;
pub mod llm_providers;
pub mod messages;
pub mod skills;
pub mod system_settings;
pub mod workspaces;

use std::{path::PathBuf, sync::Arc};

use axum::{routing::get, Router};
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
    /// Root directory for extracted skill package resources.
    pub skill_storage_root: PathBuf,
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
        .route("/api/v2/auth/register", axum::routing::post(auth::register))
        .route("/api/v2/auth/login", axum::routing::post(auth::login))
        .route("/api/v2/auth/me", get(auth::me))
        .route(
            "/api/v2/workspaces",
            axum::routing::post(workspaces::create).get(workspaces::list),
        )
        .route(
            "/api/v2/workspaces/:workspace_id",
            get(workspaces::get)
                .patch(workspaces::update)
                .delete(workspaces::delete),
        )
        .route(
            "/api/v2/agents",
            axum::routing::post(agents::create).get(agents::list),
        )
        .route("/api/v2/agents/tool-catalog", get(agents::tool_catalog))
        .route(
            "/api/v2/agents/acp-runtime-presets",
            get(agents::acp_runtime_presets),
        )
        .route(
            "/api/v2/agents/:agent_id",
            get(agents::get)
                .patch(agents::update)
                .delete(agents::delete),
        )
        .route(
            "/api/v2/llm-providers",
            axum::routing::post(llm_providers::create).get(llm_providers::list),
        )
        .route(
            "/api/v2/llm-providers/:provider_id",
            get(llm_providers::get)
                .patch(llm_providers::update)
                .delete(llm_providers::delete),
        )
        .route(
            "/api/v2/llm-providers/:provider_id/models",
            get(llm_providers::models),
        )
        .route(
            "/api/v2/settings/system",
            get(system_settings::get).patch(system_settings::update),
        )
        .route(
            "/api/v2/groups",
            axum::routing::post(groups::create).get(groups::list),
        )
        .route(
            "/api/v2/groups/:group_id",
            get(groups::get)
                .patch(groups::update)
                .delete(groups::delete),
        )
        .route(
            "/api/v2/groups/:group_id/messages/stream",
            axum::routing::post(messages::stream),
        )
        .route(
            "/api/v2/skills",
            axum::routing::post(skills::create).get(skills::list),
        )
        .route(
            "/api/v2/skills/import",
            axum::routing::post(skills::import_raw),
        )
        .route(
            "/api/v2/skills/import-package",
            axum::routing::post(skills::import_package),
        )
        .route(
            "/api/v2/skills/:skill_id",
            get(skills::get)
                .patch(skills::update)
                .delete(skills::delete),
        )
        .route(
            "/api/v2/skills/:skill_id/resources",
            get(skills::list_resources),
        )
        .route(
            "/api/v2/skills/:skill_id/resources/*resource_path",
            get(skills::read_resource).patch(skills::update_resource),
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
        skill_storage_root: std::env::temp_dir()
            .join(format!("ag-swarmer-test-skills-{}", uuid::Uuid::new_v4())),
    };
    (router(state.clone()), state)
}
