use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use axum::{http::StatusCode, routing::any, Router};
use tokio::{net::TcpListener, sync::Mutex};
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    api::{self, AppState, AuthSettings},
    config::{AppConfig, InitialUserConfig},
    db::Db,
    mcp::McpManager,
    runtime::group_scheduler::{ActiveTurnRegistry, SchedulerStore},
    terminal::TerminalManager,
};

/// Runtime options for embedding or launching the backend HTTP service.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub registration_enabled: bool,
    pub initial_user: Option<InitialUserConfig>,
    pub app_data_dir: Option<PathBuf>,
    /// Built frontend assets to serve on the same origin as the API. `None`
    /// serves the API alone.
    pub web_dir: Option<PathBuf>,
    /// Existing directory used as the first-run group workspace root.
    pub workspaces_dir: Option<PathBuf>,
}

impl From<AppConfig> for ServerConfig {
    fn from(config: AppConfig) -> Self {
        Self {
            host: config.host,
            port: config.port,
            database_url: config.database_url,
            secret_key: config.secret_key,
            access_token_expire_minutes: config.access_token_expire_minutes,
            registration_enabled: config.registration_enabled,
            initial_user: config.initial_user,
            app_data_dir: config.app_data_dir,
            web_dir: config.web_dir,
            workspaces_dir: config.workspaces_dir,
        }
    }
}

impl ServerConfig {
    pub fn skill_storage_root(&self) -> PathBuf {
        self.app_data_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(".qunica"))
            .join("skills")
    }
}

pub async fn build_state(config: &ServerConfig) -> anyhow::Result<AppState> {
    let default_group_workspace_root = match config.workspaces_dir.as_deref() {
        Some(path) => {
            let canonical = std::fs::canonicalize(path).with_context(|| {
                format!(
                    "QUNICA_WORKSPACES_DIR must be an existing directory: {}",
                    path.display()
                )
            })?;
            anyhow::ensure!(
                canonical.is_dir(),
                "QUNICA_WORKSPACES_DIR must be an existing directory: {}",
                path.display()
            );
            Some(canonical)
        }
        None => None,
    };
    let db = Db::connect(&config.database_url)
        .await
        .with_context(|| format!("failed to connect database {}", config.database_url))?;
    db.migrate().await.context("failed to run migrations")?;
    api::auth::initialize_auth(
        db.pool(),
        config.registration_enabled,
        config.initial_user.as_ref(),
    )
    .await
    .context("failed to initialize authentication")?;
    let write_lock = Arc::new(Mutex::new(()));
    SchedulerStore::new(db.pool().clone(), write_lock.clone())
        .recover_incomplete_turns()
        .await
        .context("failed to recover incomplete scheduler turns")?;

    Ok(AppState {
        db,
        auth: AuthSettings {
            secret_key: config.secret_key.clone(),
            access_token_expire_minutes: config.access_token_expire_minutes,
            registration_enabled: config.registration_enabled,
        },
        write_lock,
        active_turns: ActiveTurnRegistry::new(),
        skill_storage_root: config.skill_storage_root(),
        default_group_workspace_root,
        // The same pool the group runtime uses, so a settings edit evicts the
        // connection a turn would otherwise reuse.
        mcp: McpManager::shared(),
        terminals: TerminalManager::shared(),
    })
}

pub async fn bind_listener(config: &ServerConfig) -> anyhow::Result<(TcpListener, SocketAddr)> {
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed to read local address")?;
    Ok((listener, local_addr))
}

/// The API router, plus the single-page app when `web_dir` is set.
///
/// Serving both from one process keeps the browser on one origin, which is what
/// the CORS allowlist and the cookie-free bearer-token flow assume. Unmatched
/// paths fall back to `index.html` so client-side routes survive a reload.
pub fn build_router(state: AppState, web_dir: Option<&Path>) -> Router {
    let router = api::router(state);
    let Some(web_dir) = web_dir else {
        return router;
    };
    // `fallback`, not `not_found_service`: the latter forces the index response
    // to 404, which turns every deep-link reload into an error page.
    let index = ServeFile::new(web_dir.join("index.html"));
    router
        .route("/api", any(|| async { StatusCode::NOT_FOUND }))
        .route("/api/*path", any(|| async { StatusCode::NOT_FOUND }))
        .fallback_service(ServeDir::new(web_dir).fallback(index))
}

pub async fn serve(config: ServerConfig) -> anyhow::Result<()> {
    serve_with_shutdown(config, std::future::pending::<()>()).await
}

pub async fn serve_with_shutdown(
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let state = build_state(&config).await?;
    let (listener, addr) = bind_listener(&config).await?;
    if let Some(web_dir) = config.web_dir.as_deref() {
        tracing::info!(web_dir = %web_dir.display(), "serving frontend assets");
    }
    let router = build_router(state, config.web_dir.as_deref());
    serve_router_with_shutdown(listener, addr, router, shutdown).await
}

pub async fn serve_listener_with_shutdown(
    listener: TcpListener,
    addr: SocketAddr,
    state: AppState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    serve_router_with_shutdown(listener, addr, api::router(state), shutdown).await
}

pub async fn serve_router_with_shutdown(
    listener: TcpListener,
    addr: SocketAddr,
    router: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    tracing::info!(%addr, "qunica backend listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}
