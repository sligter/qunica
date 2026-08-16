//! ACP child-process audit persistence.
//!
//! Every ACP turn is recorded in `external_agent_runs`: a `running` row is
//! inserted when the run starts and updated to its terminal status when it
//! finishes. Task 9a provides only this audit foundation and a bounded output
//! [`Tail`]; Task 9b will spawn the actual child process, drive the ACP stdio
//! protocol, and call these helpers to persist the outcome.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
};

use serde_json::json;
use sqlx::SqlitePool;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use uuid::Uuid;

use crate::acp::config::AcpRuntimeProfile;
use crate::process::tokio_command_no_window;

/// Maximum number of characters retained in a captured stdout/stderr tail.
pub const MAX_TAIL_CHARS: usize = 12_000;

/// Marker env var set on every ACP child so a spawned agent can detect it runs
/// under ag-swarmer. Matches the Python runtime.
pub const ACP_AGENT_ENV_FLAG: &str = "AG_SWARMER_ACP_AGENT";

/// A failure while persisting an ACP audit row.
#[derive(Debug, Error)]
pub enum AcpAuditError {
    /// The underlying database operation failed.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// Serializing `argv` to JSON failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// A bounded, char-limited capture of streamed process output.
///
/// Appending always keeps only the most recent [`MAX_TAIL_CHARS`] characters
/// (or a custom limit), mirroring the Python `_Tail` helper used to retain the
/// tail end of a child process's stdout/stderr.
#[derive(Debug, Clone)]
pub struct Tail {
    limit: usize,
    value: String,
}

impl Tail {
    /// A tail bounded to [`MAX_TAIL_CHARS`] characters.
    pub fn new() -> Self {
        Self::with_limit(MAX_TAIL_CHARS)
    }

    /// A tail bounded to `limit` characters.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            value: String::new(),
        }
    }

    /// Append `text`, dropping leading characters so at most `limit` remain.
    pub fn append(&mut self, text: &str) {
        self.value.push_str(text);
        let count = self.value.chars().count();
        if count > self.limit {
            self.value = self.value.chars().skip(count - self.limit).collect();
        }
    }

    /// The current retained tail.
    pub fn snapshot(&self) -> &str {
        &self.value
    }

    /// Consume the tail, returning its retained contents.
    pub fn into_string(self) -> String {
        self.value
    }
}

impl Default for Tail {
    fn default() -> Self {
        Self::new()
    }
}

/// Identifiers and metadata for a new ACP run, captured before the child starts.
#[derive(Debug, Clone)]
pub struct AcpRunContext {
    /// Owning user id.
    pub owner_id: String,
    /// Group id, if the run is part of a group turn.
    pub group_id: Option<String>,
    /// The agent being run.
    pub agent_id: String,
    /// Thread id, if the run is bound to a thread.
    pub thread_id: Option<String>,
    /// Resolved working directory the child runs in.
    pub cwd: String,
    /// The full argv (`command` followed by `args`).
    pub argv: Vec<String>,
}

/// A live handle to one `external_agent_runs` row.
///
/// [`AcpRunAudit::start`] inserts the `running` row; one of [`complete`],
/// [`fail`], or [`cancel`] later stamps the terminal status, output tails, and
/// `ended_at`.
///
/// [`complete`]: AcpRunAudit::complete
/// [`fail`]: AcpRunAudit::fail
/// [`cancel`]: AcpRunAudit::cancel
#[derive(Clone)]
pub struct AcpRunAudit {
    pool: SqlitePool,
    id: String,
}

impl AcpRunAudit {
    /// Insert a `running` audit row for the given context and return its handle.
    pub async fn start(pool: &SqlitePool, ctx: &AcpRunContext) -> Result<Self, AcpAuditError> {
        let id = Uuid::new_v4().to_string();
        let argv_json = serde_json::to_string(&json!(ctx.argv))?;
        let started_at = now_rfc3339();

        sqlx::query(
            "INSERT INTO external_agent_runs \
             (id, owner_id, group_id, agent_id, thread_id, adapter, cwd, status, argv_json, \
              started_at) \
             VALUES (?, ?, ?, ?, ?, 'acp', ?, 'running', ?, ?)",
        )
        .bind(&id)
        .bind(&ctx.owner_id)
        .bind(&ctx.group_id)
        .bind(&ctx.agent_id)
        .bind(&ctx.thread_id)
        .bind(&ctx.cwd)
        .bind(&argv_json)
        .bind(&started_at)
        .execute(pool)
        .await?;

        Ok(Self {
            pool: pool.clone(),
            id,
        })
    }

    /// The id of the persisted run row.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Mark the run `completed` with an exit code and output tails.
    pub async fn complete(
        &self,
        exit_code: Option<i64>,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
    ) -> Result<(), AcpAuditError> {
        self.finish("completed", exit_code, stdout_tail, stderr_tail, None)
            .await
    }

    /// Mark the run `failed` with an exit code, output tails, and an error.
    pub async fn fail(
        &self,
        exit_code: Option<i64>,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
        error_message: &str,
    ) -> Result<(), AcpAuditError> {
        self.finish(
            "failed",
            exit_code,
            stdout_tail,
            stderr_tail,
            Some(error_message),
        )
        .await
    }

    /// Mark the run `cancelled` with output tails and an error message. A
    /// cancelled run has no meaningful exit code.
    pub async fn cancel(
        &self,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
        error_message: &str,
    ) -> Result<(), AcpAuditError> {
        self.finish(
            "cancelled",
            None,
            stdout_tail,
            stderr_tail,
            Some(error_message),
        )
        .await
    }

    async fn finish(
        &self,
        status: &str,
        exit_code: Option<i64>,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<(), AcpAuditError> {
        let ended_at = now_rfc3339();
        sqlx::query(
            "UPDATE external_agent_runs \
             SET status = ?, exit_code = ?, stdout_tail = ?, stderr_tail = ?, error_message = ?, \
                 ended_at = ? \
             WHERE id = ?",
        )
        .bind(status)
        .bind(exit_code)
        .bind(bound_tail(stdout_tail))
        .bind(bound_tail(stderr_tail))
        .bind(error_message)
        .bind(&ended_at)
        .bind(&self.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// Bound an optional output tail to the most recent [`MAX_TAIL_CHARS`] chars.
fn bound_tail(value: Option<&str>) -> Option<String> {
    value.map(|text| {
        let count = text.chars().count();
        if count <= MAX_TAIL_CHARS {
            text.to_string()
        } else {
            text.chars().skip(count - MAX_TAIL_CHARS).collect()
        }
    })
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Host environment keys that carry a CLI's auth/config location. For the
/// `codex`/`claude`/`pi` profiles these are inherited so the agent can reuse the
/// host user's existing login; for untrusted profiles they are instead
/// redirected to an isolated temp tree (see [`acp_agent_env`]). Mirrors the
/// Python `_host_cli_auth_env`.
fn host_cli_auth_env(profile: AcpRuntimeProfile) -> Vec<(String, String)> {
    let mut keys: Vec<&str> = vec![
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ];
    match profile {
        AcpRuntimeProfile::Codex => keys.push("CODEX_HOME"),
        AcpRuntimeProfile::Claude => {
            keys.extend(["CLAUDE_CONFIG_DIR", "CLAUDE_HOME", "ANTHROPIC_MODEL"]);
        }
        AcpRuntimeProfile::Custom
        | AcpRuntimeProfile::Pi
        | AcpRuntimeProfile::Opencode
        | AcpRuntimeProfile::Dsh => {}
    }
    keys.into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect()
}

/// Environment prepared specifically for a short-lived capability probe.
///
/// A probe must not inherit the backend's full environment. `variables`
/// contains only process-launch essentials, an isolated home/temp tree,
/// profile-specific authentication variables, and the runtime's explicit
/// environment. `sensitive_values` is retained so probe output can be checked
/// before it crosses the API boundary.
pub(super) struct ProbeChildEnv {
    pub variables: Vec<(String, String)>,
    pub sensitive_values: Vec<String>,
}

/// Build the minimal environment used by capability-discovery children.
pub(super) fn build_probe_child_env(
    profile: AcpRuntimeProfile,
    isolated_home: &Path,
    runtime_env: &BTreeMap<String, String>,
) -> io::Result<ProbeChildEnv> {
    const PROCESS_LAUNCH_KEYS: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
    ];
    const CODEX_AUTH_KEYS: &[&str] = &["CODEX_HOME", "CODEX_API_KEY", "OPENAI_API_KEY"];
    const CLAUDE_AUTH_KEYS: &[&str] = &[
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_HOME",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
    ];

    let mut env = BTreeMap::new();
    for (key, value) in std::env::vars() {
        if PROCESS_LAUNCH_KEYS
            .iter()
            .any(|allowed| key.eq_ignore_ascii_case(allowed))
        {
            env.insert(key, value);
        }
    }

    let config_dir = isolated_home.join("config");
    let data_dir = isolated_home.join("data");
    let cache_dir = isolated_home.join("cache");
    let temp_dir = isolated_home.join("tmp");
    let codex_dir = config_dir.join("codex");
    let claude_dir = config_dir.join("claude");
    for dir in [
        isolated_home,
        &config_dir,
        &data_dir,
        &cache_dir,
        &temp_dir,
        &codex_dir,
        &claude_dir,
    ] {
        std::fs::create_dir_all(dir)?;
    }
    let s = |path: &Path| path.to_string_lossy().to_string();
    for (key, value) in [
        ("HOME", s(isolated_home)),
        ("USERPROFILE", s(isolated_home)),
        ("APPDATA", s(&config_dir)),
        ("LOCALAPPDATA", s(&data_dir)),
        ("XDG_CONFIG_HOME", s(&config_dir)),
        ("XDG_DATA_HOME", s(&data_dir)),
        ("XDG_CACHE_HOME", s(&cache_dir)),
        ("TMP", s(&temp_dir)),
        ("TEMP", s(&temp_dir)),
        ("TMPDIR", s(&temp_dir)),
        ("CODEX_HOME", s(&codex_dir)),
        ("CLAUDE_CONFIG_DIR", s(&claude_dir)),
        ("CLAUDE_HOME", s(&claude_dir)),
    ] {
        env.insert(key.to_string(), value);
    }
    env.insert(ACP_AGENT_ENV_FLAG.to_string(), "1".to_string());

    // Known CLI profiles need the same authenticated home as an actual agent
    // run. Custom, OpenCode, and dsh adapters remain fully isolated.
    if matches!(
        profile,
        AcpRuntimeProfile::Codex | AcpRuntimeProfile::Claude | AcpRuntimeProfile::Pi
    ) {
        // If the host has no explicit CODEX_HOME/CLAUDE_* override, omit the
        // isolated value so the CLI uses its normal location below HOME.
        match profile {
            AcpRuntimeProfile::Codex => {
                env.remove("CODEX_HOME");
            }
            AcpRuntimeProfile::Claude => {
                env.remove("CLAUDE_CONFIG_DIR");
                env.remove("CLAUDE_HOME");
            }
            AcpRuntimeProfile::Custom
            | AcpRuntimeProfile::Pi
            | AcpRuntimeProfile::Opencode
            | AcpRuntimeProfile::Dsh => {}
        }
        for (key, value) in host_cli_auth_env(profile) {
            env.insert(key, value);
        }
    }

    let auth_keys = match profile {
        AcpRuntimeProfile::Codex => CODEX_AUTH_KEYS,
        AcpRuntimeProfile::Claude => CLAUDE_AUTH_KEYS,
        AcpRuntimeProfile::Custom
        | AcpRuntimeProfile::Pi
        | AcpRuntimeProfile::Opencode
        | AcpRuntimeProfile::Dsh => &[],
    };
    let mut sensitive_values = Vec::new();
    for key in auth_keys {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                sensitive_values.push(value.clone());
            }
            env.insert((*key).to_string(), value);
        }
    }

    for (key, value) in runtime_env {
        if !value.is_empty() {
            sensitive_values.push(value.clone());
        }
        env.insert(key.clone(), value.clone());
    }

    Ok(ProbeChildEnv {
        variables: env.into_iter().collect(),
        sensitive_values,
    })
}

/// Build the ACP-specific environment overlay for a child, mirroring the Python
/// `_acp_agent_env`.
///
/// Always sets [`ACP_AGENT_ENV_FLAG`]. For `codex`/`claude`/`pi` it inherits the
/// host CLI auth env then applies the runtime env. For untrusted profiles it points
/// every home/config/data/cache key at an isolated tree rooted under `isolated_home`
/// (created here) so the agent cannot read or poison the host user's CLI state,
/// then applies the runtime env. The runtime env is applied last; the blocked
/// keys it could otherwise use to override these are already rejected by config
/// normalization, so it can only add benign keys.
fn acp_agent_env(
    profile: AcpRuntimeProfile,
    isolated_home: &Path,
    runtime_env: &BTreeMap<String, String>,
) -> io::Result<BTreeMap<String, String>> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(ACP_AGENT_ENV_FLAG.to_string(), "1".to_string());

    match profile {
        AcpRuntimeProfile::Codex | AcpRuntimeProfile::Claude | AcpRuntimeProfile::Pi => {
            for (key, value) in host_cli_auth_env(profile) {
                env.insert(key, value);
            }
        }
        AcpRuntimeProfile::Custom | AcpRuntimeProfile::Opencode | AcpRuntimeProfile::Dsh => {
            let config_dir = isolated_home.join("config");
            let data_dir = isolated_home.join("data");
            let cache_dir = isolated_home.join("cache");
            for dir in [isolated_home, &config_dir, &data_dir, &cache_dir] {
                std::fs::create_dir_all(dir)?;
            }
            let s = |path: &Path| path.to_string_lossy().to_string();
            env.insert("HOME".to_string(), s(isolated_home));
            env.insert("USERPROFILE".to_string(), s(isolated_home));
            env.insert("APPDATA".to_string(), s(&config_dir));
            env.insert("LOCALAPPDATA".to_string(), s(&data_dir));
            env.insert("XDG_CONFIG_HOME".to_string(), s(&config_dir));
            env.insert("XDG_DATA_HOME".to_string(), s(&data_dir));
            env.insert("XDG_CACHE_HOME".to_string(), s(&cache_dir));
            env.insert("CODEX_HOME".to_string(), s(&config_dir.join("codex")));
            env.insert(
                "CLAUDE_CONFIG_DIR".to_string(),
                s(&config_dir.join("claude")),
            );
            env.insert("CLAUDE_HOME".to_string(), s(&config_dir.join("claude")));
            if profile == AcpRuntimeProfile::Dsh {
                // dsh resolves its own state directory from DSH_HOME; without
                // this it would fall back to the launch cwd, i.e. the user's
                // workspace.
                env.insert("DSH_HOME".to_string(), s(&config_dir.join("dsh")));
            }
        }
    }

    for (key, value) in runtime_env {
        env.insert(key.clone(), value.clone());
    }
    Ok(env)
}

/// Build the full child environment: the inherited process env as a base, with
/// the ACP overlay from [`acp_agent_env`] applied on top.
///
/// Using the process env as the base supplies `PATH`/`SystemRoot`/etc.; the
/// overlay then redirects the home/config keys (for `custom`) so host
/// credential stores are never reachable through an inherited `HOME`.
pub fn build_child_env(
    profile: AcpRuntimeProfile,
    isolated_home: &Path,
    runtime_env: &BTreeMap<String, String>,
) -> io::Result<Vec<(String, String)>> {
    let mut env: BTreeMap<String, String> = std::env::vars().collect();
    for (key, value) in acp_agent_env(profile, isolated_home, runtime_env)? {
        env.insert(key, value);
    }
    Ok(env.into_iter().collect())
}

/// A spawned ACP child with its stdio pipes taken out for the protocol layer.
pub struct SpawnedAcpChild {
    /// The running child handle (used to wait for exit or kill on timeout).
    pub child: Child,
    /// The child's stdin (JSON-RPC requests are written here).
    pub stdin: ChildStdin,
    /// The child's stdout (JSON-RPC responses/notifications are read here).
    pub stdout: ChildStdout,
    /// The child's stderr (captured into a bounded tail for the audit row).
    pub stderr: ChildStderr,
}

/// Spawn an ACP agent child process with piped stdio, the given environment,
/// `cwd` as the working directory, and (on Windows) no console window.
///
/// The environment is set explicitly via `env_clear` + the supplied pairs, so
/// the child sees exactly what [`build_child_env`] computed.
pub fn spawn_acp_child(
    command: &str,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
) -> io::Result<SpawnedAcpChild> {
    let (launch_command, launch_args) = windows_batch_launch_command(command, args);
    let mut std_cmd = StdCommand::new(launch_command);
    std_cmd.args(launch_args).current_dir(cwd).env_clear();
    for (key, value) in env {
        std_cmd.env(key, value);
    }
    std_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut cmd = tokio_command_no_window(std_cmd);
    cmd.kill_on_drop(true);
    let mut child = cmd.spawn()?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("ACP child stdin pipe missing"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("ACP child stdout pipe missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("ACP child stderr pipe missing"))?;

    Ok(SpawnedAcpChild {
        child,
        stdin,
        stdout,
        stderr,
    })
}

/// Windows npm globally-installed executables are `.cmd` shims. Resolve the
/// package's JavaScript entrypoint and invoke Node directly so the ACP process
/// does not rely on Windows batch-file command-line semantics.
fn windows_batch_launch_command(command: &str, args: &[String]) -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    {
        if matches!(
            Path::new(command)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some(extension) if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        ) {
            if let Some((node, entrypoint)) = npm_cmd_entrypoint(command) {
                let mut launch_args = vec![entrypoint.to_string_lossy().into_owned()];
                launch_args.extend(args.iter().cloned());
                return (node, launch_args);
            }
            let comspec = std::env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
            let mut launch_args = vec!["/d".to_string(), "/c".to_string(), "call".to_string()];
            launch_args.push(command.to_string());
            launch_args.extend(args.iter().cloned());
            return (comspec, launch_args);
        }
    }

    (PathBuf::from(command), args.to_vec())
}

#[cfg(windows)]
fn npm_cmd_entrypoint(command: &str) -> Option<(PathBuf, PathBuf)> {
    let npm_bin_dir = Path::new(command).parent()?;
    let package_name = Path::new(command).file_stem()?.to_str()?;
    let entrypoint = npm_bin_dir
        .join("node_modules")
        .join("@agentclientprotocol")
        .join(package_name)
        .join("dist")
        .join("index.js");
    let node = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join("node.exe"))
            .find(|candidate| candidate.is_file())
    })?;
    entrypoint.is_file().then_some((node, entrypoint))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{acp_agent_env, build_probe_child_env, windows_batch_launch_command};
    use crate::acp::AcpRuntimeProfile;

    #[test]
    fn pi_reuses_host_cli_locations_for_runs_and_probes() {
        let isolated = tempfile::tempdir().unwrap();
        let runtime_env = BTreeMap::new();
        let run_env = acp_agent_env(AcpRuntimeProfile::Pi, isolated.path(), &runtime_env).unwrap();
        let probe_env: BTreeMap<_, _> =
            build_probe_child_env(AcpRuntimeProfile::Pi, isolated.path(), &runtime_env)
                .unwrap()
                .variables
                .into_iter()
                .collect();
        let mut inherited = 0;

        for key in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
            let Ok(host_value) = std::env::var(key) else {
                continue;
            };
            inherited += 1;
            assert_eq!(run_env.get(key), Some(&host_value));
            assert_eq!(probe_env.get(key), Some(&host_value));
        }
        assert!(inherited > 0, "test host must expose a CLI home location");
    }

    #[cfg(windows)]
    #[test]
    fn wraps_unknown_batch_shims_with_cmd_call() {
        let (command, args) = windows_batch_launch_command(
            r"C:\Users\Test\missing.cmd",
            &["--flag".to_string(), "value with spaces".to_string()],
        );

        assert!(command.to_string_lossy().ends_with("cmd.exe"));
        assert_eq!(
            args,
            vec![
                "/d",
                "/c",
                "call",
                r"C:\Users\Test\missing.cmd",
                "--flag",
                "value with spaces",
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn leaves_regular_commands_unchanged() {
        let (command, args) = windows_batch_launch_command("codex-acp", &["--flag".to_string()]);
        assert_eq!(command.to_string_lossy(), "codex-acp");
        assert_eq!(args, vec!["--flag"]);
    }
}
