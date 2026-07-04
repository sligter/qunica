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
    path::Path,
    process::{Command as StdCommand, Stdio},
};

use serde_json::json;
use sqlx::SqlitePool;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command as TokioCommand};
use uuid::Uuid;

use crate::acp::config::AcpRuntimeProfile;

/// Maximum number of characters retained in a captured stdout/stderr tail.
pub const MAX_TAIL_CHARS: usize = 12_000;

/// Marker env var set on every ACP child so a spawned agent can detect it runs
/// under ag-swarmer. Matches the Python runtime.
pub const ACP_AGENT_ENV_FLAG: &str = "AG_SWARMER_ACP_AGENT";

/// `CreateNoWindow` process-creation flag, so a Windows GUI session does not
/// flash a console window when spawning a CLI agent.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
/// `codex`/`claude` profiles these are inherited so the agent can reuse the
/// host user's existing login; for generic profiles they are instead
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
        AcpRuntimeProfile::Custom | AcpRuntimeProfile::Pi | AcpRuntimeProfile::Opencode => {}
    }
    keys.into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect()
}

/// Build the ACP-specific environment overlay for a child, mirroring the Python
/// `_acp_agent_env`.
///
/// Always sets [`ACP_AGENT_ENV_FLAG`]. For `codex`/`claude` it inherits the host
/// CLI auth env then applies the runtime env. For generic profiles it points
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
        AcpRuntimeProfile::Codex | AcpRuntimeProfile::Claude => {
            for (key, value) in host_cli_auth_env(profile) {
                env.insert(key, value);
            }
        }
        AcpRuntimeProfile::Custom | AcpRuntimeProfile::Pi | AcpRuntimeProfile::Opencode => {
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
    let mut std_cmd = StdCommand::new(command);
    std_cmd.args(args).current_dir(cwd).env_clear();
    for (key, value) in env {
        std_cmd.env(key, value);
    }
    std_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut cmd = TokioCommand::from(std_cmd);
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
