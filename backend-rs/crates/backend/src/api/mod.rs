pub mod agents;
pub mod app_actions;
pub mod assistant;
pub mod auth;
pub mod conversations;
pub mod direct_chats;
pub mod error;
pub mod group_turns;
pub mod groups;
pub mod health;
pub mod llm_providers;
pub mod mcp_servers;
pub mod messages;
pub mod skills;
mod sse_replay;
pub mod system_settings;
pub mod threads;
pub mod token_usage;
pub mod workspace_files;
pub mod workspaces;

use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::DefaultBodyLimit,
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
        HeaderName, HeaderValue, Method,
    },
    routing::get,
    Router,
};
use tokio::sync::Mutex;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{db::Db, mcp::McpManager, runtime::group_scheduler::ActiveTurnRegistry};

/// Shared application state injected into every API v2 handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub auth: AuthSettings,
    /// Serializes chat/runtime writes so per-thread sequence allocation stays
    /// atomic across concurrent streams on the same SQLite database.
    pub write_lock: Arc<Mutex<()>>,
    /// Process-local cancellation handles for scheduler turns that are still
    /// executing. Durable turn state remains the source of truth.
    pub active_turns: ActiveTurnRegistry,
    /// Root directory for extracted skill package resources.
    pub skill_storage_root: PathBuf,
    /// Deployment-provided root applied only while first-run setup is incomplete.
    pub default_group_workspace_root: Option<PathBuf>,
    /// Pooled MCP connections, shared with the group runtime so editing a
    /// server's row can evict the connection the runtime would otherwise reuse.
    pub mcp: Arc<McpManager>,
}

/// Auth configuration needed to mint and verify access tokens.
#[derive(Clone)]
pub struct AuthSettings {
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub registration_enabled: bool,
}

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            is_allowed_origin(origin)
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            ACCEPT,
            HeaderName::from_static("last-event-id"),
        ]);

    Router::new()
        .route("/api/v1/health", get(health::health))
        .route("/api/v2/health", get(health::health))
        .route("/api/v2/auth/config", get(auth::public_config))
        .route("/api/v2/auth/register", axum::routing::post(auth::register))
        .route("/api/v2/auth/login", axum::routing::post(auth::login))
        .route("/api/v2/auth/me", get(auth::me).patch(auth::update_me))
        .route("/api/v2/token-usage", get(token_usage::get))
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
            "/api/v2/agents/system-prompt/generate",
            axum::routing::post(agents::generate_system_prompt),
        )
        .route(
            "/api/v2/assistant",
            get(assistant::get).patch(assistant::update),
        )
        .route(
            "/api/v2/app-actions",
            get(app_actions::list).delete(app_actions::clear),
        )
        .route(
            "/api/v2/app-actions/:action_id",
            axum::routing::delete(app_actions::delete),
        )
        .route(
            "/api/v2/app-actions/:action_id/approve",
            axum::routing::post(app_actions::approve),
        )
        .route(
            "/api/v2/app-actions/:action_id/reject",
            axum::routing::post(app_actions::reject),
        )
        .route(
            "/api/v2/agents/acp-runtime-presets",
            get(agents::acp_runtime_presets),
        )
        .route(
            "/api/v2/agents/acp-runtime-versions",
            get(agents::acp_runtime_versions),
        )
        .route(
            "/api/v2/agents/acp-runtime-versions/:preset_id/install",
            axum::routing::post(agents::install_acp_runtime_version),
        )
        .route(
            "/api/v2/agents/acp-runtime-capabilities",
            axum::routing::post(agents::acp_runtime_capabilities),
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
            "/api/v2/llm-providers/discover-models",
            axum::routing::post(llm_providers::discover),
        )
        .route(
            "/api/v2/llm-providers/test-model",
            axum::routing::post(llm_providers::test_model),
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
            "/api/v2/mcp-servers",
            axum::routing::post(mcp_servers::create).get(mcp_servers::list),
        )
        .route(
            "/api/v2/mcp-servers/test",
            axum::routing::post(mcp_servers::test_draft),
        )
        .route(
            "/api/v2/mcp-servers/:server_id",
            get(mcp_servers::get)
                .patch(mcp_servers::update)
                .delete(mcp_servers::delete),
        )
        .route(
            "/api/v2/mcp-servers/:server_id/test",
            axum::routing::post(mcp_servers::test_connection),
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
            "/api/v2/group-templates",
            axum::routing::post(groups::create_group_template).get(groups::list_group_templates),
        )
        .route(
            "/api/v2/group-templates/:template_id",
            axum::routing::delete(groups::delete_group_template),
        )
        .route(
            "/api/v2/direct-chats",
            axum::routing::post(direct_chats::create).get(direct_chats::list),
        )
        .route(
            "/api/v2/direct-chats/:chat_id",
            get(direct_chats::get)
                .patch(direct_chats::update)
                .delete(direct_chats::delete),
        )
        .route(
            "/api/v2/groups/:group_id",
            get(groups::get)
                .patch(groups::update)
                .delete(groups::delete),
        )
        .route(
            "/api/v2/groups/:group_id/prompt/enhance",
            axum::routing::post(groups::enhance_group_prompt),
        )
        .route(
            "/api/v2/groups/:group_id/members",
            axum::routing::post(groups::add_group_member).get(groups::list_group_members),
        )
        .route(
            "/api/v2/groups/:group_id/member-candidates",
            get(groups::search_group_member_candidates),
        )
        .route(
            "/api/v2/groups/:group_id/notes",
            axum::routing::post(groups::create_group_note).get(groups::list_group_notes),
        )
        .route(
            "/api/v2/groups/:group_id/notes/:note_id",
            get(groups::get_group_note)
                .patch(groups::update_group_note)
                .delete(groups::delete_group_note),
        )
        .route(
            "/api/v2/groups/:group_id/files",
            axum::routing::post(groups::upload_group_file).get(groups::list_group_files),
        )
        .route(
            "/api/v2/groups/:group_id/files/:file_id",
            axum::routing::delete(groups::delete_group_file),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files",
            get(groups::list_group_workspace_files).delete(groups::delete_workspace_file_route),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/root",
            get(groups::get_group_workspace_root),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/preview",
            get(groups::preview_group_workspace_file),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/upload",
            axum::routing::post(groups::upload_workspace_file_route)
                .layer(DefaultBodyLimit::max(26 * 1024 * 1024)),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/download",
            get(groups::download_group_workspace_file),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/text",
            get(groups::read_group_workspace_file_text),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/text/save",
            axum::routing::patch(groups::save_group_workspace_file_text),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/create",
            axum::routing::post(groups::create_workspace_entry_route),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/rename",
            axum::routing::patch(groups::rename_workspace_file_route),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-files/actions",
            axum::routing::post(groups::workspace_file_actions_route),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-roots",
            get(groups::list_workspace_roots_route),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/status",
            get(groups::get_group_workspace_git_status),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/branches",
            get(groups::get_group_workspace_git_branches)
                .post(groups::create_group_workspace_git_branch),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/branches/switch",
            axum::routing::post(groups::switch_group_workspace_git_branch),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/branches/rename",
            axum::routing::post(groups::rename_group_workspace_git_branch),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/branches/delete",
            axum::routing::post(groups::delete_group_workspace_git_branch),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/init",
            axum::routing::post(groups::init_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/fetch",
            axum::routing::post(groups::fetch_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/set-remote",
            axum::routing::post(groups::set_group_workspace_git_remote),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/discard",
            axum::routing::post(groups::discard_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/ignore",
            axum::routing::post(groups::ignore_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/stash/push",
            axum::routing::post(groups::stash_push_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/stash/pop",
            axum::routing::post(groups::stash_pop_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/diff",
            get(groups::get_group_workspace_git_diff),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/log",
            get(groups::get_group_workspace_git_log),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/commits/:sha",
            get(groups::get_group_workspace_git_commit),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/commits/:sha/diff",
            get(groups::get_group_workspace_git_commit_diff),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/commits/:sha/create-branch",
            axum::routing::post(groups::create_group_workspace_git_branch_from_commit),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/stage",
            axum::routing::post(groups::stage_group_workspace_git_paths),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/unstage",
            axum::routing::post(groups::unstage_group_workspace_git_paths),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/commit-message",
            axum::routing::post(groups::generate_group_workspace_git_commit_message),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/commit",
            axum::routing::post(groups::commit_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/pull",
            axum::routing::post(groups::pull_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/push",
            axum::routing::post(groups::push_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/force-push",
            axum::routing::post(groups::force_push_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/workspace-git/rebase",
            axum::routing::post(groups::rebase_group_workspace_git),
        )
        .route(
            "/api/v2/groups/:group_id/members/:user_id",
            axum::routing::delete(groups::remove_group_member),
        )
        .route(
            "/api/v2/groups/:group_id/members/:user_id/mute",
            axum::routing::patch(groups::set_group_member_muted),
        )
        .route(
            "/api/v2/groups/:group_id/agents",
            axum::routing::post(groups::add_group_agent).get(groups::list_group_agents),
        )
        .route(
            "/api/v2/groups/:group_id/agents/:agent_id",
            axum::routing::delete(groups::remove_group_agent),
        )
        .route(
            "/api/v2/groups/:group_id/agents/:agent_id/mute",
            axum::routing::patch(groups::set_group_agent_muted),
        )
        .route(
            "/api/v2/groups/:group_id/agents/:agent_id/topology",
            axum::routing::patch(groups::set_group_agent_topology),
        )
        .route(
            "/api/v2/groups/:group_id/agents/:agent_id/workspace-sharing",
            axum::routing::patch(groups::set_group_agent_workspace_sharing),
        )
        .route(
            "/api/v2/groups/:group_id/turns/:turn_id",
            get(group_turns::get),
        )
        .route(
            "/api/v2/groups/:group_id/turns/:turn_id/cancel",
            axum::routing::post(group_turns::cancel),
        )
        .route(
            "/api/v2/groups/:group_id/messages",
            axum::routing::post(messages::send_group).get(messages::list_group),
        )
        .route(
            "/api/v2/groups/:group_id/messages/clear",
            axum::routing::post(messages::clear_group),
        )
        .route(
            "/api/v2/groups/:group_id/context/reset",
            axum::routing::post(messages::reset_group_context),
        )
        .route(
            "/api/v2/groups/:group_id/messages/:message_id",
            axum::routing::delete(messages::delete_group),
        )
        .route(
            "/api/v2/groups/:group_id/messages/stream",
            axum::routing::post(messages::stream_group),
        )
        .route(
            "/api/v2/groups/:group_id/threads",
            axum::routing::post(threads::create_group).get(threads::list_group),
        )
        .route(
            "/api/v2/direct-chats/:group_id/messages",
            axum::routing::post(messages::send_direct).get(messages::list_direct),
        )
        .route(
            "/api/v2/direct-chats/:group_id/messages/clear",
            axum::routing::post(messages::clear_direct),
        )
        .route(
            "/api/v2/direct-chats/:group_id/context/reset",
            axum::routing::post(messages::reset_direct_context),
        )
        .route(
            "/api/v2/direct-chats/:group_id/messages/:message_id",
            axum::routing::delete(messages::delete_direct),
        )
        .route(
            "/api/v2/direct-chats/:group_id/messages/stream",
            axum::routing::post(messages::stream_direct),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files",
            get(direct_chats::list_workspace_files).delete(direct_chats::delete_workspace_file),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/upload",
            axum::routing::post(direct_chats::upload_workspace_file)
                .layer(DefaultBodyLimit::max(26 * 1024 * 1024)),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/create",
            axum::routing::post(direct_chats::create_workspace_entry),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/rename",
            axum::routing::patch(direct_chats::rename_workspace_file),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/actions",
            axum::routing::post(direct_chats::workspace_file_actions),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-roots",
            get(direct_chats::list_workspace_roots),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/root",
            get(direct_chats::get_workspace_root),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/preview",
            get(direct_chats::preview_workspace_file),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/download",
            get(direct_chats::download_workspace_file),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/text",
            get(direct_chats::read_workspace_file_text),
        )
        .route(
            "/api/v2/direct-chats/:chat_id/workspace-files/text/save",
            axum::routing::patch(direct_chats::save_workspace_file_text),
        )
        .route(
            "/api/v2/threads/:thread_id",
            get(threads::get).delete(threads::delete),
        )
        .route(
            "/api/v2/threads/:thread_id/cancel",
            axum::routing::post(threads::cancel),
        )
        .route(
            "/api/v2/threads/:thread_id/resume",
            axum::routing::post(threads::resume),
        )
        .route(
            "/api/v2/threads/:thread_id/archive",
            axum::routing::post(threads::archive),
        )
        .route(
            "/api/v2/threads/:thread_id/unarchive",
            axum::routing::post(threads::unarchive),
        )
        .route(
            "/api/v2/threads/:thread_id/messages/clear",
            axum::routing::post(threads::clear_messages),
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
            "/api/v2/skills/import-github",
            axum::routing::post(skills::import_github),
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
        .layer(cors)
}

fn is_allowed_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    matches!(
        origin,
        "http://tauri.localhost"
            | "https://tauri.localhost"
            | "tauri://localhost"
            | "http://localhost"
            | "http://127.0.0.1"
    ) || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:")
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
            registration_enabled: true,
        },
        write_lock: Arc::new(Mutex::new(())),
        active_turns: ActiveTurnRegistry::new(),
        skill_storage_root: std::env::temp_dir()
            .join(format!("qunica-test-skills-{}", uuid::Uuid::new_v4())),
        default_group_workspace_root: None,
        // A private pool per test router: the shared one would carry live
        // connections between tests that each build their own database.
        mcp: Arc::new(McpManager::new()),
    };
    (router(state.clone()), state)
}
