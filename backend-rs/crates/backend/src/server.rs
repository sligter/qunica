use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context;
use tokio::{net::TcpListener, sync::Mutex};

use crate::{
    api::{self, AppState, AuthSettings},
    config::AppConfig,
    db::Db,
    mcp::McpManager,
    runtime::group_scheduler::{ActiveTurnRegistry, SchedulerStore},
};

/// Runtime options for embedding or launching the backend HTTP service.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub app_data_dir: Option<PathBuf>,
}

impl From<AppConfig> for ServerConfig {
    fn from(config: AppConfig) -> Self {
        Self {
            host: config.host,
            port: config.port,
            database_url: config.database_url,
            secret_key: config.secret_key,
            access_token_expire_minutes: config.access_token_expire_minutes,
            app_data_dir: config.app_data_dir,
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
    let db = Db::connect(&config.database_url)
        .await
        .with_context(|| format!("failed to connect database {}", config.database_url))?;
    db.migrate().await.context("failed to run migrations")?;
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
        },
        write_lock,
        active_turns: ActiveTurnRegistry::new(),
        skill_storage_root: config.skill_storage_root(),
        // The same pool the group runtime uses, so a settings edit evicts the
        // connection a turn would otherwise reuse.
        mcp: McpManager::shared(),
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

pub async fn serve(config: ServerConfig) -> anyhow::Result<()> {
    serve_with_shutdown(config, std::future::pending::<()>()).await
}

pub async fn serve_with_shutdown(
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let state = build_state(&config).await?;
    let (listener, addr) = bind_listener(&config).await?;
    serve_listener_with_shutdown(listener, addr, state, shutdown).await
}

pub async fn serve_listener_with_shutdown(
    listener: TcpListener,
    addr: SocketAddr,
    state: AppState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    tracing::info!(%addr, "qunica backend listening");
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}
