use anyhow::Context;
use qunica_backend::{config::AppConfig, server, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env_and_args()?;
    telemetry::setup_tracing(&config).context("failed to initialize tracing")?;
    server::serve(config.into()).await?;
    Ok(())
}
