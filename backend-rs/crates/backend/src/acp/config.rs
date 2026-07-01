//! ACP runtime config normalization.
//!
//! This mirrors the Python oracle `backend/app/external_agents/adapters.py`:
//! an agent's stored `external_runtime_json` is untrusted input that must be
//! normalized into an [`AcpRuntimeConfig`] before any ACP child process is
//! launched (Task 9b). Normalization rejects host-environment leaks, legacy CLI
//! adapter fields, and malformed values, and applies the same defaults and
//! ranges as the Python implementation so both backends accept and reject the
//! exact same configs.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use thiserror::Error;

/// Default per-turn timeout applied when a config omits `timeout_seconds`.
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 3600;
/// Upper bound for `timeout_seconds` (6 hours).
pub const MAX_TIMEOUT_SECONDS: u32 = 6 * 60 * 60;

/// Environment keys an ACP runtime config may never override.
///
/// These point processes at host credential/config stores; allowing an agent
/// config to set them would let an untrusted runtime read or poison the host
/// user's CLI auth state. Task 9b is responsible for supplying isolated values
/// for these keys itself.
pub const BLOCKED_ENV_KEYS: [&str; 12] = [
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_HOME",
    "AG_SWARMER_EXTERNAL_AGENT",
    "AG_SWARMER_ACP_AGENT",
];

/// How the ACP client answers permission requests from the agent process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionPolicy {
    /// Deny every permission request (the safe default).
    #[default]
    Deny,
    /// Auto-allow the first allow option offered for each request.
    AutoAllow,
}

impl PermissionPolicy {
    /// The wire string for this policy, matching the Python literal values.
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionPolicy::Deny => "deny",
            PermissionPolicy::AutoAllow => "auto_allow",
        }
    }
}

/// A known agent runtime profile, selecting host-auth and session-setting
/// behavior in Task 9b.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AcpRuntimeProfile {
    /// A generic ACP agent run with an isolated home (no host credential leak).
    #[default]
    Custom,
    /// The `codex` CLI profile.
    Codex,
    /// The `claude` CLI profile.
    Claude,
}

impl AcpRuntimeProfile {
    /// The wire string for this profile, matching the Python literal values.
    pub fn as_str(&self) -> &'static str {
        match self {
            AcpRuntimeProfile::Custom => "custom",
            AcpRuntimeProfile::Codex => "codex",
            AcpRuntimeProfile::Claude => "claude",
        }
    }
}

/// A single ACP config-option value: a string or a boolean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpConfigValue {
    /// A string option value (trimmed, non-empty, control-char-free).
    Str(String),
    /// A boolean option value.
    Bool(bool),
}

/// A normalized, ready-to-launch ACP runtime configuration.
///
/// Field semantics mirror the Python `AcpRuntimeConfig` dataclass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpRuntimeConfig {
    /// The executable to spawn (trimmed, non-empty, control-char-free).
    pub command: String,
    /// Positional arguments passed to the command (each NUL-free).
    pub args: Vec<String>,
    /// Extra environment for the child process (NUL-free keys/values; no
    /// blocked keys).
    pub env: BTreeMap<String, String>,
    /// Per-turn timeout in seconds, in `1..=MAX_TIMEOUT_SECONDS`.
    pub timeout_seconds: u32,
    /// Permission-request handling policy.
    pub permission_policy: PermissionPolicy,
    /// Runtime profile selecting host-auth/session behavior.
    pub profile: AcpRuntimeProfile,
    /// Optional model id to apply to the session.
    pub model: Option<String>,
    /// Optional session mode to apply.
    pub mode: Option<String>,
    /// Optional thinking/reasoning effort to apply.
    pub thinking_effort: Option<String>,
    /// Optional extra session config options (string or bool values).
    pub config_options: Option<BTreeMap<String, AcpConfigValue>>,
}

/// A config that failed normalization. The message is safe to surface and
/// matches the Python `AgentChatError` text for the same condition.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct AcpConfigError(pub String);

fn invalid(message: impl Into<String>) -> AcpConfigError {
    AcpConfigError(message.into())
}

/// Normalize an agent's raw ACP runtime config into an [`AcpRuntimeConfig`].
///
/// `raw` is the parsed `external_runtime_json`. A missing config, a non-object
/// config, or any malformed/forbidden field is rejected with an
/// [`AcpConfigError`] whose message matches the Python oracle.
pub fn normalize_acp_runtime(raw: Option<&Value>) -> Result<AcpRuntimeConfig, AcpConfigError> {
    let map = match raw {
        Some(Value::Object(map)) => map,
        _ => return Err(invalid("ACP runtime config is required for ACP agents")),
    };

    reject_legacy_external_cli(map)?;

    let command = normalize_required_text(map.get("command"), "ACP runtime command")?;
    let profile = normalize_profile(map.get("profile"))?;
    let args = normalize_args(map.get("args"))?;
    let env = normalize_env(map.get("env"))?;
    let timeout_seconds = normalize_timeout(map.get("timeout_seconds"))?;
    let permission_policy = normalize_permission_policy(map.get("permission_policy"))?;
    let config_options = normalize_config_options(map.get("config_options"))?;
    let model = normalize_optional_text(map.get("model"), "ACP runtime model")?;
    let mode = normalize_optional_text(map.get("mode"), "ACP runtime mode")?;
    let thinking_effort =
        normalize_optional_text(map.get("thinking_effort"), "ACP runtime thinking_effort")?;

    Ok(AcpRuntimeConfig {
        command,
        args,
        env,
        timeout_seconds,
        permission_policy,
        profile,
        model,
        mode,
        thinking_effort,
        config_options,
    })
}

/// Reject the deprecated external-CLI `adapter` field. A legacy value gets the
/// migration hint; any other non-null value is rejected outright.
fn reject_legacy_external_cli(map: &Map<String, Value>) -> Result<(), AcpConfigError> {
    match map.get("adapter") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(s)) if s == "codex" || s == "claude_code" => Err(invalid(
            "external CLI adapters are deprecated; configure this agent with an ACP \
             runtime command instead",
        )),
        Some(_) => Err(invalid(
            "ACP runtime config must not include an adapter field",
        )),
    }
}

fn normalize_required_text(value: Option<&Value>, label: &str) -> Result<String, AcpConfigError> {
    match value {
        Some(Value::String(s)) => normalize_required_str(s, label),
        _ => Err(invalid(format!("{label} is required"))),
    }
}

fn normalize_required_str(value: &str, label: &str) -> Result<String, AcpConfigError> {
    let text = value.trim();
    if text.is_empty() {
        return Err(invalid(format!("{label} is required")));
    }
    reject_control_chars(text, label)?;
    Ok(text.to_string())
}

fn normalize_optional_text(
    value: Option<&Value>,
    label: &str,
) -> Result<Option<String>, AcpConfigError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let text = s.trim();
            if text.is_empty() {
                return Ok(None);
            }
            reject_control_chars(text, label)?;
            Ok(Some(text.to_string()))
        }
        Some(_) => Err(invalid(format!("{label} must be a string"))),
    }
}

fn normalize_profile(value: Option<&Value>) -> Result<AcpRuntimeProfile, AcpConfigError> {
    match value {
        None | Some(Value::Null) => Ok(AcpRuntimeProfile::Custom),
        Some(Value::String(s)) => match s.trim() {
            "custom" => Ok(AcpRuntimeProfile::Custom),
            "codex" => Ok(AcpRuntimeProfile::Codex),
            "claude" => Ok(AcpRuntimeProfile::Claude),
            _ => Err(invalid(
                "ACP runtime profile must be custom, codex, or claude",
            )),
        },
        Some(_) => Err(invalid("ACP runtime profile must be a string")),
    }
}

fn normalize_args(value: Option<&Value>) -> Result<Vec<String>, AcpConfigError> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut args = Vec::with_capacity(items.len());
            for item in items {
                let text = item
                    .as_str()
                    .ok_or_else(|| invalid("ACP runtime args must be a list of strings"))?;
                reject_nul(text, "ACP runtime arg")?;
                args.push(text.to_string());
            }
            Ok(args)
        }
        Some(_) => Err(invalid("ACP runtime args must be a list of strings")),
    }
}

fn normalize_env(value: Option<&Value>) -> Result<BTreeMap<String, String>, AcpConfigError> {
    match value {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(map)) => {
            let mut env = BTreeMap::new();
            for (key, raw_value) in map {
                let val = match raw_value {
                    Value::String(s) => s,
                    _ => return Err(invalid("ACP runtime env keys and values must be strings")),
                };
                if BLOCKED_ENV_KEYS.contains(&key.as_str()) {
                    return Err(invalid(format!("ACP runtime env may not override {key}")));
                }
                reject_nul(key, "ACP runtime env key")?;
                reject_nul(val, "ACP runtime env value")?;
                env.insert(key.clone(), val.clone());
            }
            Ok(env)
        }
        Some(_) => Err(invalid("ACP runtime env must be an object")),
    }
}

fn normalize_timeout(value: Option<&Value>) -> Result<u32, AcpConfigError> {
    // Mirror Python's `int(raw.get("timeout_seconds") or DEFAULT)`: a missing
    // value or a literal 0 falls back to the default; any other value is used
    // as-is and then range-checked.
    let seconds: i64 = match value {
        None | Some(Value::Null) => DEFAULT_TIMEOUT_SECONDS as i64,
        Some(v) => {
            let n = v
                .as_i64()
                .or_else(|| v.as_f64().map(|f| f as i64))
                .ok_or_else(|| invalid("ACP runtime timeout_seconds is out of range"))?;
            if n == 0 {
                DEFAULT_TIMEOUT_SECONDS as i64
            } else {
                n
            }
        }
    };
    if seconds < 1 || seconds > MAX_TIMEOUT_SECONDS as i64 {
        return Err(invalid("ACP runtime timeout_seconds is out of range"));
    }
    Ok(seconds as u32)
}

fn normalize_permission_policy(value: Option<&Value>) -> Result<PermissionPolicy, AcpConfigError> {
    let policy = match value {
        None | Some(Value::Null) => "deny",
        Some(Value::String(s)) if s.is_empty() => "deny",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => {
            return Err(invalid(
                "ACP runtime permission_policy must be deny or auto_allow",
            ))
        }
    };
    match policy {
        "deny" => Ok(PermissionPolicy::Deny),
        "auto_allow" => Ok(PermissionPolicy::AutoAllow),
        _ => Err(invalid(
            "ACP runtime permission_policy must be deny or auto_allow",
        )),
    }
}

fn normalize_config_options(
    value: Option<&Value>,
) -> Result<Option<BTreeMap<String, AcpConfigValue>>, AcpConfigError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => {
            let mut options = BTreeMap::new();
            for (key, raw_value) in map {
                if key.trim().is_empty() {
                    return Err(invalid("ACP runtime config option keys must be strings"));
                }
                let normalized_key = normalize_required_str(key, "ACP runtime config option key")?;
                match raw_value {
                    Value::Bool(b) => {
                        options.insert(normalized_key, AcpConfigValue::Bool(*b));
                    }
                    Value::String(s) => {
                        let normalized_value =
                            normalize_required_str(s, "ACP runtime config option value")?;
                        options.insert(normalized_key, AcpConfigValue::Str(normalized_value));
                    }
                    _ => {
                        return Err(invalid(
                            "ACP runtime config option values must be strings or booleans",
                        ))
                    }
                }
            }
            Ok(if options.is_empty() {
                None
            } else {
                Some(options)
            })
        }
        Some(_) => Err(invalid("ACP runtime config_options must be an object")),
    }
}

fn reject_control_chars(value: &str, label: &str) -> Result<(), AcpConfigError> {
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(invalid(format!("{label} is invalid")));
    }
    Ok(())
}

fn reject_nul(value: &str, label: &str) -> Result<(), AcpConfigError> {
    if value.contains('\0') {
        return Err(invalid(format!("{label} is invalid")));
    }
    Ok(())
}
