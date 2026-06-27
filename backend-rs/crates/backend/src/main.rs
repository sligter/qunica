use ag_swarmer_backend::{
    api::{self, AppState, AuthSettings},
    config::AppConfig,
    db::Db,
    telemetry,
};
use anyhow::Context;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env_and_args()?;
    telemetry::setup_tracing(&config).context("failed to initialize tracing")?;

    let db = Db::connect(&config.database_url)
        .await
        .with_context(|| format!("failed to connect database {}", config.database_url))?;
    db.migrate().await.context("failed to run migrations")?;

    let state = AppState {
        db,
        auth: AuthSettings {
            secret_key: config.secret_key.clone(),
            access_token_expire_minutes: config.access_token_expire_minutes,
        },
        write_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        skill_storage_root: config
            .app_data_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(".ag-swarmer"))
            .join("skills"),
    };

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "ag-swarmer backend listening");
    axum::serve(listener, api::router(state)).await?;
    Ok(())
}
