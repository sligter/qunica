use clap::Parser;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Parser)]
pub struct AppConfig {
    #[arg(long, env = "AG_SWARMER_HOST", default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, env = "AG_SWARMER_PORT", default_value_t = 8765)]
    pub port: u16,
    #[arg(long, env = "AG_SWARMER_APP_DATA")]
    pub app_data_dir: Option<PathBuf>,
    #[arg(long, env = "AG_SWARMER_LOG_LEVEL", default_value = "info")]
    pub log_level: String,
    #[arg(
        long,
        env = "AG_SWARMER_DATABASE_URL",
        default_value = "sqlite://ag-swarmer.db?mode=rwc"
    )]
    pub database_url: String,
    #[arg(
        long,
        env = "SECRET_KEY",
        default_value = "please-change-me-in-production"
    )]
    pub secret_key: String,
    #[arg(long, env = "ACCESS_TOKEN_EXPIRE_MINUTES", default_value_t = 10080)]
    pub access_token_expire_minutes: i64,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Clap(#[from] clap::Error),
}

impl AppConfig {
    pub fn from_env_and_args() -> Result<Self, ConfigError> {
        Self::try_parse().map_err(ConfigError::Clap)
    }
}
