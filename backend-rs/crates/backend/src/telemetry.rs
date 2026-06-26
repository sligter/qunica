use crate::config::AppConfig;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error(transparent)]
    Init(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
}

pub fn setup_tracing(config: &AppConfig) -> Result<(), TelemetryError> {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).try_init()?;
    Ok(())
}
