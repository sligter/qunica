use clap::Parser;
use std::path::PathBuf;
use std::{ffi::OsStr, fmt, fs, path::Path};
use thiserror::Error;

const DEFAULT_DATABASE_URL: &str = "sqlite://qunica.db?mode=rwc";
const DEFAULT_SECRET_KEY: &str = "please-change-me-in-production";

#[derive(Clone)]
pub struct InitialUserConfig {
    pub email: String,
    pub password: String,
    pub name: String,
}

impl fmt::Debug for InitialUserConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InitialUserConfig")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .field("name", &self.name)
            .finish()
    }
}

#[derive(Debug, Clone, Parser)]
pub struct AppConfig {
    #[arg(long, env = "QUNICA_HOST", default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, env = "QUNICA_PORT", default_value_t = 8765)]
    pub port: u16,
    #[arg(long, env = "QUNICA_APP_DATA")]
    pub app_data_dir: Option<PathBuf>,
    /// Directory of built frontend assets to serve alongside the API. Unset
    /// means API only, which is what the desktop shell wants.
    #[arg(long, env = "QUNICA_WEB_DIR")]
    pub web_dir: Option<PathBuf>,
    /// Existing directory suggested as the group workspace root during first
    /// run. The Docker image creates and configures `/workspaces` here.
    #[arg(long, env = "QUNICA_WORKSPACES_DIR")]
    pub workspaces_dir: Option<PathBuf>,
    #[arg(long, env = "QUNICA_LOG_LEVEL", default_value = "info")]
    pub log_level: String,
    #[arg(
        long,
        env = "QUNICA_DATABASE_URL",
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
    #[arg(
        long,
        env = "QUNICA_REGISTRATION_ENABLED",
        default_value_t = true,
        action = clap::ArgAction::Set
    )]
    pub registration_enabled: bool,
    #[arg(long, env = "QUNICA_INITIAL_USER_EMAIL")]
    initial_user_email: Option<String>,
    #[arg(long, env = "QUNICA_INITIAL_USER_PASSWORD")]
    initial_user_password: Option<String>,
    #[arg(long, env = "QUNICA_INITIAL_USER_NAME")]
    initial_user_name: Option<String>,
    #[arg(skip)]
    pub initial_user: Option<InitialUserConfig>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Clap(#[from] clap::Error),
    #[error("failed to initialize app data config: {0}")]
    AppData(#[from] std::io::Error),
    #[error("QUNICA_INITIAL_USER_EMAIL and QUNICA_INITIAL_USER_PASSWORD must be set together")]
    IncompleteInitialUser,
}

impl AppConfig {
    pub fn from_env_and_args() -> Result<Self, ConfigError> {
        let mut config = Self::try_parse().map_err(ConfigError::Clap)?;
        // ACP children inherit the server environment; keep the one-time
        // bootstrap password out of every shell and external agent process.
        std::env::remove_var("QUNICA_INITIAL_USER_PASSWORD");
        config.initial_user = initial_user_from_parts(
            config.initial_user_email.take(),
            config.initial_user_password.take(),
            config.initial_user_name.take(),
        )?;
        config.apply_app_data_defaults()?;
        if config.web_dir.is_none() {
            config.web_dir = packaged_web_dir();
        }
        Ok(config)
    }

    pub fn for_desktop_app_data(app_data_dir: PathBuf, port: u16) -> Result<Self, ConfigError> {
        std::env::remove_var("QUNICA_INITIAL_USER_PASSWORD");
        let log_level = std::env::var("QUNICA_LOG_LEVEL")
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
            // The desktop shell serves the UI from its own webview bundle.
            web_dir: None,
            workspaces_dir: None,
            log_level,
            database_url: std::env::var("QUNICA_DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
            secret_key: std::env::var("SECRET_KEY")
                .unwrap_or_else(|_| DEFAULT_SECRET_KEY.to_string()),
            access_token_expire_minutes: std::env::var("ACCESS_TOKEN_EXPIRE_MINUTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(10080),
            registration_enabled: true,
            initial_user_email: None,
            initial_user_password: None,
            initial_user_name: None,
            initial_user: None,
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
            && std::env::var_os("QUNICA_DATABASE_URL").is_none()
            && !arg_present("database-url")
        {
            self.database_url = sqlite_url_for_path(&app_data_dir.join("qunica.sqlite3"));
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

fn initial_user_from_parts(
    email: Option<String>,
    password: Option<String>,
    name: Option<String>,
) -> Result<Option<InitialUserConfig>, ConfigError> {
    let email = email.filter(|value| !value.is_empty());
    let password = password.filter(|value| !value.is_empty());
    let name = name.filter(|value| !value.is_empty());
    match (email, password, name) {
        (None, None, None) => Ok(None),
        (Some(email), Some(password), name) => Ok(Some(InitialUserConfig {
            email,
            password,
            name: name.unwrap_or_else(|| "Admin".to_string()),
        })),
        _ => Err(ConfigError::IncompleteInitialUser),
    }
}

/// The `web/` directory the server release archive stages next to the binary.
///
/// Only counts when it holds an `index.html`, so a `cargo run` from a directory
/// that happens to contain an empty `web/` still starts as an API-only server.
fn packaged_web_dir() -> Option<PathBuf> {
    let candidate = std::env::current_exe().ok()?.parent()?.join("web");
    web_dir_if_populated(candidate)
}

fn web_dir_if_populated(candidate: PathBuf) -> Option<PathBuf> {
    candidate.join("index.html").is_file().then_some(candidate)
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
    use super::{
        initial_user_from_parts, read_or_create_desktop_secret, sqlite_url_for_path,
        web_dir_if_populated,
    };

    #[test]
    fn config_app_data_sqlite_url_uses_desktop_database_name() {
        let path = std::path::Path::new("C:/Users/Test/AppData/Roaming/qunica.desktop")
            .join("qunica.sqlite3");
        let url = sqlite_url_for_path(&path);
        assert!(url.starts_with("sqlite://"));
        assert!(url.ends_with("/qunica.sqlite3?mode=rwc"));
        assert!(url.contains("qunica.desktop"));
    }

    #[test]
    fn config_desktop_secret_is_generated_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let first = read_or_create_desktop_secret(dir.path()).unwrap();
        let second = read_or_create_desktop_secret(dir.path()).unwrap();
        assert_eq!(first, second);
        assert!(dir.path().join("desktop-secret.key").is_file());
    }

    #[test]
    fn config_packaged_web_dir_needs_an_index_html() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("web");
        std::fs::create_dir_all(&web).unwrap();
        assert_eq!(web_dir_if_populated(web.clone()), None);

        std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
        assert_eq!(web_dir_if_populated(web.clone()), Some(web));
    }

    #[test]
    fn config_initial_user_requires_email_and_password() {
        assert!(
            initial_user_from_parts(Some("".into()), Some("".into()), Some("".into()))
                .unwrap()
                .is_none()
        );
        assert!(initial_user_from_parts(Some("admin@example.com".into()), None, None).is_err());

        let user = initial_user_from_parts(
            Some("admin@example.com".into()),
            Some("long-password".into()),
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(user.name, "Admin");
    }
}
