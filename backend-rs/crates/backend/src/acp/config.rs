//! ACP runtime config normalization.
//!
//! An agent's stored `external_runtime_json` is untrusted input that must be
//! normalized into an [`AcpRuntimeConfig`] before any ACP child process is
//! launched (Task 9b). Normalization rejects host-environment leaks, legacy CLI
//! adapter fields, and malformed values, and applies stable defaults and ranges
//! for the Rust ACP runtime.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AcpRuntimeProfile {
    /// A generic ACP agent run with an isolated home (no host credential leak).
    #[default]
    Custom,
    /// The `codex` CLI profile.
    Codex,
    /// The `claude` CLI profile.
    Claude,
    /// The Pi Agent ACP adapter profile.
    Pi,
    /// The OpenCode ACP server profile.
    Opencode,
}

impl AcpRuntimeProfile {
    /// The wire string for this profile, matching the Python literal values.
    pub fn as_str(&self) -> &'static str {
        match self {
            AcpRuntimeProfile::Custom => "custom",
            AcpRuntimeProfile::Codex => "codex",
            AcpRuntimeProfile::Claude => "claude",
            AcpRuntimeProfile::Pi => "pi",
            AcpRuntimeProfile::Opencode => "opencode",
        }
    }
}

/// A single ACP config-option value: a string or a boolean.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AcpConfigValue {
    /// A string option value (trimmed, non-empty, control-char-free).
    Str(String),
    /// A boolean option value.
    Bool(bool),
}

/// A normalized, ready-to-launch ACP runtime configuration.
///
/// Field semantics mirror the Python `AcpRuntimeConfig` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
/// [`AcpConfigError`] with a stable user-facing message.
pub fn normalize_acp_runtime(raw: Option<&Value>) -> Result<AcpRuntimeConfig, AcpConfigError> {
    let map = match raw {
        Some(Value::Object(map)) => map,
        _ => return Err(invalid("ACP runtime config is required for ACP agents")),
    };

    reject_legacy_external_cli(map)?;

    let command = normalize_required_text(map.get("command"), "ACP runtime command")?;
    let profile = normalize_profile(map.get("profile"))?;
    let mut args = normalize_args(map.get("args"))?;
    let env = normalize_env(map.get("env"))?;
    let timeout_seconds = normalize_timeout(map.get("timeout_seconds"))?;
    let permission_policy = normalize_permission_policy(map.get("permission_policy"))?;
    let config_options = normalize_config_options(map.get("config_options"))?;
    let model = normalize_optional_text(map.get("model"), "ACP runtime model")?;
    let mode = normalize_optional_text(map.get("mode"), "ACP runtime mode")?;
    let thinking_effort =
        normalize_optional_text(map.get("thinking_effort"), "ACP runtime thinking_effort")?;

    migrate_legacy_codex_acp_command(profile, &command, &mut args);

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

/// Resolve installed ACP executables for saved `npx` configurations.
///
/// Package resolution can exceed the capability-probe deadline. Prefer the
/// already-installed executable whenever it is available on the host PATH.
pub fn canonicalize_acp_runtime(config: &mut AcpRuntimeConfig) {
    let executable = match config.profile {
        AcpRuntimeProfile::Codex => Some("codex-acp"),
        AcpRuntimeProfile::Pi => Some("pi-acp"),
        _ => None,
    };
    let command = executable
        .and_then(find_command_on_path)
        .map(|path| path.to_string_lossy().into_owned());
    canonicalize_acp_runtime_with_command(config, command.as_deref());
}

fn canonicalize_acp_runtime_with_command(
    config: &mut AcpRuntimeConfig,
    installed_command: Option<&str>,
) {
    let package = match config.profile {
        AcpRuntimeProfile::Codex if is_codex_acp_config(config) => {
            Some("@agentclientprotocol/codex-acp")
        }
        AcpRuntimeProfile::Pi => Some("pi-acp"),
        _ => None,
    };
    if package.is_some_and(|package| is_npx_package_config(config, package)) {
        if let Some(command) = installed_command {
            config.command = command.to_string();
            config.args.clear();
        }
    }

    if package.is_some() && config.profile == AcpRuntimeProfile::Codex {
        config.mode = match config.mode.as_deref() {
            Some("auto") => Some("agent".to_string()),
            Some("full-access") => Some("agent-full-access".to_string()),
            _ => config.mode.take(),
        };
    }
}

fn is_codex_acp_config(config: &AcpRuntimeConfig) -> bool {
    is_codex_acp_npx_config(config)
        || Path::new(&config.command)
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("codex-acp"))
}

fn is_codex_acp_npx_config(config: &AcpRuntimeConfig) -> bool {
    is_npx_package_config(config, "@agentclientprotocol/codex-acp")
}

fn is_npx_package_config(config: &AcpRuntimeConfig, package: &str) -> bool {
    let command_name = Path::new(&config.command)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&config.command);
    command_name.eq_ignore_ascii_case("npx") && config.args.iter().any(|arg| arg == package)
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_env = env::var_os("PATH")?;
    let candidates = command_candidates(command, env::var_os("PATHEXT"));
    env::split_paths(&path_env)
        .flat_map(|dir| candidates.iter().map(move |candidate| dir.join(candidate)))
        .find(|path| is_executable_file(path))
        .map(absolute_command_path)
}

fn command_candidates(command: &str, pathext: Option<OsString>) -> Vec<String> {
    #[cfg(windows)]
    {
        let has_extension = Path::new(command).extension().is_some();
        if has_extension {
            return vec![command.to_string()];
        }

        let extensions = pathext
            .as_deref()
            .and_then(|value| value.to_str())
            .unwrap_or(".COM;.EXE;.BAT;.CMD");
        extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| format!("{command}{extension}"))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = pathext;
        vec![command.to_string()]
    }
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(windows)]
    {
        path.is_file()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && path
                .metadata()
                .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }
}

fn absolute_command_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    }
}

fn migrate_legacy_codex_acp_command(
    profile: AcpRuntimeProfile,
    command: &str,
    args: &mut Vec<String>,
) {
    if profile != AcpRuntimeProfile::Codex
        || !args.iter().any(|arg| arg == "@zed-industries/codex-acp")
    {
        return;
    }

    args.retain(|arg| arg != "@zed-industries/codex-acp");
    if !args.iter().any(|arg| arg == "-y") {
        args.insert(0, "-y".to_string());
    }
    args.push("@agentclientprotocol/codex-acp".to_string());
    if command.eq_ignore_ascii_case("codex-acp") {
        args.clear();
    }
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
            "pi" => Ok(AcpRuntimeProfile::Pi),
            "opencode" => Ok(AcpRuntimeProfile::Opencode),
            _ => Err(invalid(
                "ACP runtime profile must be custom, codex, claude, pi, or opencode",
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{canonicalize_acp_runtime_with_command, normalize_acp_runtime, AcpRuntimeProfile};

    #[test]
    fn canonicalizes_saved_npx_codex_adapter_and_legacy_modes() {
        let mut config = normalize_acp_runtime(Some(&json!({
            "profile": "codex",
            "command": "C:/Program Files/nodejs/npx.cmd",
            "args": ["-y", "@agentclientprotocol/codex-acp"],
            "mode": "full-access",
        })))
        .expect("saved Codex configuration normalizes");

        canonicalize_acp_runtime_with_command(&mut config, Some("C:/Users/test/npm/codex-acp.cmd"));

        assert_eq!(config.profile, AcpRuntimeProfile::Codex);
        assert_eq!(config.command, "C:/Users/test/npm/codex-acp.cmd");
        assert!(config.args.is_empty());
        assert_eq!(config.mode.as_deref(), Some("agent-full-access"));
    }

    #[test]
    fn keeps_npx_fallback_when_local_codex_adapter_is_missing() {
        let mut config = normalize_acp_runtime(Some(&json!({
            "profile": "codex",
            "command": "npx",
            "args": ["-y", "@agentclientprotocol/codex-acp"],
            "mode": "auto",
        })))
        .expect("fallback Codex configuration normalizes");

        canonicalize_acp_runtime_with_command(&mut config, None);

        assert_eq!(config.command, "npx");
        assert_eq!(config.args, vec!["-y", "@agentclientprotocol/codex-acp"]);
        assert_eq!(config.mode.as_deref(), Some("agent"));
    }

    #[test]
    fn canonicalizes_saved_npx_pi_adapter() {
        let mut config = normalize_acp_runtime(Some(&json!({
            "profile": "pi",
            "command": "C:/Program Files/nodejs/npx.cmd",
            "args": ["-y", "pi-acp"],
        })))
        .expect("saved Pi configuration normalizes");

        canonicalize_acp_runtime_with_command(&mut config, Some("C:/Users/test/npm/pi-acp.cmd"));

        assert_eq!(config.command, "C:/Users/test/npm/pi-acp.cmd");
        assert!(config.args.is_empty());
    }
}
