use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tracing_subscriber::{
    fmt, layer::SubscriberExt, reload, util::SubscriberInitExt, EnvFilter, Registry,
};

use crate::config::AppConfig;

const APPLICATION_LOG: &str = "application.jsonl";
const FILTER_FILE: &str = "log-filter.txt";
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

type FilterHandle = reload::Handle<EnvFilter, Registry>;

struct LogControl {
    reload: FilterHandle,
    current: RwLock<String>,
    log_dir: PathBuf,
    log_path: PathBuf,
    filter_path: PathBuf,
}

static LOG_CONTROL: OnceLock<LogControl> = OnceLock::new();

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Init(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
}

#[derive(Debug, Error)]
pub enum LogControlError {
    #[error("system logging is unavailable")]
    Unavailable,
    #[error("invalid log filter: {0}")]
    InvalidFilter(String),
    #[error("failed to update log filter: {0}")]
    Reload(String),
    #[error("system log state is unavailable")]
    Lock,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Serialize)]
pub struct SystemLogEntry {
    timestamp: String,
    level: String,
    target: String,
    message: String,
    fields: Value,
}

#[derive(Debug, Serialize)]
pub struct SystemLogSnapshot {
    filter: String,
    log_dir: String,
    entries: Vec<SystemLogEntry>,
}

pub fn setup_tracing(config: &AppConfig) -> Result<(), TelemetryError> {
    let log_dir = config
        .app_data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(".ag-swarmer"))
        .join("logs");
    fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join(APPLICATION_LOG);
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let requested = config.log_level.trim();
    let (filter, current) = EnvFilter::try_new(requested)
        .map(|filter| (filter, requested.to_string()))
        .unwrap_or_else(|_| (EnvFilter::new("info"), "info".to_string()));
    let (filter_layer, reload) = reload::Layer::new(filter);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt::layer())
        .with(fmt::layer().json().with_ansi(false).with_writer(log_file))
        .try_init()
        .map_err(|error| TelemetryError::Init(Box::new(error)))?;

    let _ = LOG_CONTROL.set(LogControl {
        reload,
        current: RwLock::new(current),
        filter_path: log_dir.join(FILTER_FILE),
        log_dir,
        log_path,
    });
    Ok(())
}

pub fn log_snapshot(limit: usize) -> Result<SystemLogSnapshot, LogControlError> {
    let control = control()?;
    let filter = control
        .current
        .read()
        .map_err(|_| LogControlError::Lock)?
        .clone();
    Ok(SystemLogSnapshot {
        filter,
        log_dir: control.log_dir.to_string_lossy().into_owned(),
        entries: read_recent_logs_from_path(&control.log_path, limit.min(1_000))?,
    })
}

pub fn set_log_filter(filter: &str) -> Result<(), LogControlError> {
    let normalized = filter.trim();
    let normalized = if normalized.is_empty() {
        "info"
    } else {
        normalized
    };
    let parsed = EnvFilter::try_new(normalized)
        .map_err(|error| LogControlError::InvalidFilter(error.to_string()))?;
    let control = control()?;
    fs::write(&control.filter_path, format!("{normalized}\n"))?;
    control
        .reload
        .reload(parsed)
        .map_err(|error| LogControlError::Reload(error.to_string()))?;
    *control.current.write().map_err(|_| LogControlError::Lock)? = normalized.to_string();
    Ok(())
}

pub fn clear_logs() -> Result<(), LogControlError> {
    let control = control()?;
    for name in [APPLICATION_LOG, "backend.log", "launcher.log"] {
        let path = control.log_dir.join(name);
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
    }
    Ok(())
}

fn control() -> Result<&'static LogControl, LogControlError> {
    LOG_CONTROL.get().ok_or(LogControlError::Unavailable)
}

fn read_recent_logs_from_path(
    path: &Path,
    limit: usize,
) -> Result<Vec<SystemLogEntry>, std::io::Error> {
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }

    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if length > MAX_READ_BYTES {
        file.seek(SeekFrom::Start(length - MAX_READ_BYTES))?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut entries = text
        .lines()
        .rev()
        .filter_map(parse_log_entry)
        .take(limit)
        .collect::<Vec<_>>();
    entries.reverse();
    Ok(entries)
}

fn parse_log_entry(line: &str) -> Option<SystemLogEntry> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let fields = value
        .get("fields")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let message = fields
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(SystemLogEntry {
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        level: value
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        target: value
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        message,
        fields,
    })
}

#[cfg(test)]
mod tests {
    use super::read_recent_logs_from_path;

    #[test]
    fn reads_newest_json_log_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("application.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"timestamp":"2026-07-29T12:00:00Z","level":"INFO","fields":{"message":"first"},"target":"ag_swarmer"}"#,
                "\n",
                r#"{"timestamp":"2026-07-29T12:00:01Z","level":"WARN","fields":{"message":"second","code":2},"target":"ag_swarmer::api"}"#,
                "\n",
            ),
        )
        .unwrap();

        let entries = read_recent_logs_from_path(&path, 1).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "second");
        assert_eq!(entries[0].level, "WARN");
        assert_eq!(entries[0].target, "ag_swarmer::api");
    }
}
