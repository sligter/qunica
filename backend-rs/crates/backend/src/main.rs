mod config;
mod telemetry;

use anyhow::Context;
use config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env_and_args()?;
    telemetry::setup_tracing(&config).context("failed to initialize tracing")?;
    tracing::info!(host = %config.host, port = config.port, "starting ag-swarmer backend");
    Ok(())
}
