use clap::Parser;
use std::path::PathBuf;
use std::{ffi::OsStr, fs, path::Path};
use thiserror::Error;

const DEFAULT_DATABASE_URL: &str = "sqlite://ag-swarmer.db?mode=rwc";
const DEFAULT_SECRET_KEY: &str = "please-change-me-in-production";

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
        default_value = DEFAULT_DATABASE_URL
    )]
    pub database_url: String,
    #[arg(
        long,
        env = "SECRET_KEY",
        default_value = DEFAULT_SECRET_KEY
    )]
    pub secret_key: String,
    #[arg(long, env = "ACCESS_TOKEN_EXPIRE_MINUTES", default_value_t = 10080)]
    pub access_token_expire_minutes: i64,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Clap(#[from] clap::Error),
    #[error("failed to initialize app data config: {0}")]
    AppData(#[from] std::io::Error),
}

impl AppConfig {
    pub fn from_env_and_args() -> Result<Self, ConfigError> {
        let mut config = Self::try_parse().map_err(ConfigError::Clap)?;
        config.apply_app_data_defaults()?;
        Ok(config)
    }

    pub fn for_desktop_app_data(app_data_dir: PathBuf, port: u16) -> Result<Self, ConfigError> {
        let log_level = std::env::var("AG_SWARMER_LOG_LEVEL")
            .ok()
            .or_else(|| {
                fs::read_to_string(app_data_dir.join("logs").join("log-filter.txt"))
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| "info".to_string());
        let mut config = Self {
            host: "127.0.0.1".to_string(),
            port,
            app_data_dir: Some(app_data_dir),
            log_level,
            database_url: std::env::var("AG_SWARMER_DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
            secret_key: std::env::var("SECRET_KEY")
                .unwrap_or_else(|_| DEFAULT_SECRET_KEY.to_string()),
            access_token_expire_minutes: std::env::var("ACCESS_TOKEN_EXPIRE_MINUTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(10080),
        };
        config.apply_app_data_defaults()?;
        Ok(config)
    }

    fn apply_app_data_defaults(&mut self) -> Result<(), ConfigError> {
        let Some(app_data_dir) = self.app_data_dir.clone() else {
            return Ok(());
        };
        fs::create_dir_all(&app_data_dir)?;

        if self.database_url == DEFAULT_DATABASE_URL
            && std::env::var_os("AG_SWARMER_DATABASE_URL").is_none()
            && !arg_present("database-url")
        {
            self.database_url = sqlite_url_for_path(&app_data_dir.join("ag-swarmer.sqlite3"));
        }

        if self.secret_key == DEFAULT_SECRET_KEY
            && std::env::var_os("SECRET_KEY").is_none()
            && !arg_present("secret-key")
        {
            self.secret_key = read_or_create_desktop_secret(&app_data_dir)?;
        }

        Ok(())
    }
}

fn arg_present(name: &str) -> bool {
    let long = format!("--{name}");
    let prefix = format!("{long}=");
    std::env::args_os()
        .any(|arg| arg == OsStr::new(&long) || arg.to_string_lossy().starts_with(&prefix))
}

fn sqlite_url_for_path(path: &Path) -> String {
    let mut display = path.to_string_lossy().replace('\\', "/");
    if !display.starts_with('/') && !display.starts_with("//") {
        display = format!("/{display}");
    }
    format!("sqlite://{display}?mode=rwc")
}

fn read_or_create_desktop_secret(app_data_dir: &Path) -> Result<String, std::io::Error> {
    let path = app_data_dir.join("desktop-secret.key");
    match fs::read_to_string(&path) {
        Ok(existing) if !existing.trim().is_empty() => return Ok(existing.trim().to_string()),
        Ok(_) | Err(_) => {}
    }
    let secret = format!(
        "{}{}{}{}",
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4()
    );
    fs::write(&path, format!("{secret}\n"))?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::{read_or_create_desktop_secret, sqlite_url_for_path};

    #[test]
    fn config_app_data_sqlite_url_uses_desktop_database_name() {
        let path = std::path::Path::new("C:/Users/Test/AppData/Roaming/dev.ag-swarmer.desktop")
            .join("ag-swarmer.sqlite3");
        let url = sqlite_url_for_path(&path);
        assert!(url.starts_with("sqlite://"));
        assert!(url.ends_with("/ag-swarmer.sqlite3?mode=rwc"));
        assert!(url.contains("dev.ag-swarmer.desktop"));
    }

    #[test]
    fn config_desktop_secret_is_generated_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let first = read_or_create_desktop_secret(dir.path()).unwrap();
        let second = read_or_create_desktop_secret(dir.path()).unwrap();
        assert_eq!(first, second);
        assert!(dir.path().join("desktop-secret.key").is_file());
    }
}
