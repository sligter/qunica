use ag_swarmer_backend::{
    api::{self, AppState},
    config::AppConfig,
    telemetry,
};
use anyhow::Context;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env_and_args()?;
    telemetry::setup_tracing(&config).context("failed to initialize tracing")?;
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "ag-swarmer backend listening");
    axum::serve(listener, api::router(AppState)).await?;
    Ok(())
}
