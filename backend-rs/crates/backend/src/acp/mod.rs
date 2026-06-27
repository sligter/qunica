//! ACP (Agent Client Protocol) runtime.
//!
//! [`config`] normalizes an agent's raw ACP runtime config into a validated
//! [`AcpRuntimeConfig`]; [`process`] holds the `external_agent_runs` audit
//! helpers, the bounded output [`Tail`], the isolated-environment builder, and
//! the child-process spawner; [`protocol`] is the newline-delimited JSON-RPC
//! stdio transport.
//!
//! [`run_acp_agent_stream`] ties them together: it inserts a `running` audit
//! row, spawns the configured agent, drives the ACP session (`initialize` →
//! `session/new` → settings → `session/prompt`), streams [`AcpAgentEvent`]s
//! mapped from `session/update` notifications, and persists the terminal audit
//! state on normal completion, timeout, or cancellation. Wiring this into the
//! group/direct runtimes is deferred to a later task.

pub mod config;
pub mod process;
pub mod protocol;

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use serde_json::{json, Value};
use sqlx::SqlitePool;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
    sync::{mpsc, Notify},
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

pub use config::{
    normalize_acp_runtime, AcpConfigError, AcpConfigValue, AcpRuntimeConfig, AcpRuntimeProfile,
    PermissionPolicy, BLOCKED_ENV_KEYS, DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS,
};
pub use process::{
    build_child_env, spawn_acp_child, AcpAuditError, AcpRunAudit, AcpRunContext, SpawnedAcpChild,
    Tail, ACP_AGENT_ENV_FLAG, MAX_TAIL_CHARS,
};
use protocol::{
    AcpConnection, ProtocolError, METHOD_INITIALIZE, METHOD_SESSION_CANCEL, METHOD_SESSION_NEW,
    METHOD_SESSION_PROMPT, METHOD_SESSION_SET_CONFIG_OPTION, METHOD_SESSION_SET_MODE,
    METHOD_SESSION_SET_MODEL, PROTOCOL_VERSION,
};

/// How long to wait for a child to exit (cleanly or after a kill) before
/// abandoning the wait and reporting no exit code.
const CHILD_EXIT_GRACE: Duration = Duration::from_secs(5);
/// How long to wait for the stdout protocol reader to drain after child exit.
const STDOUT_DRAIN_GRACE: Duration = Duration::from_secs(2);
/// How long to wait for the stderr-capture task to drain after the child exits.
const STDERR_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// The kind of a streamed ACP event. Mirrors the Python `AcpAgentEvent.kind`
/// string values so a future group runtime can map them one-for-one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpEventKind {
    /// Run lifecycle marker (`running` first, then a terminal status).
    Run,
    /// A chunk of agent message text.
    Token,
    /// A chunk of agent reasoning/thought text.
    Reasoning,
    /// A tool call began.
    ToolCallStart,
    /// A tool call produced a result/progress update.
    ToolCallResult,
    /// A token-usage update for the turn.
    Usage,
}

impl AcpEventKind {
    /// The wire string for this kind, matching the Python literals.
    pub fn as_str(&self) -> &'static str {
        match self {
            AcpEventKind::Run => "run",
            AcpEventKind::Token => "token",
            AcpEventKind::Reasoning => "reasoning",
            AcpEventKind::ToolCallStart => "tool_call_start",
            AcpEventKind::ToolCallResult => "tool_call_result",
            AcpEventKind::Usage => "usage",
        }
    }
}

/// A single event streamed from an ACP run.
///
/// `data` is a JSON string for [`AcpEventKind::Token`]/[`AcpEventKind::Reasoning`]
/// and a JSON object for the other kinds, mirroring the Python
/// `AcpAgentEvent.data: str | dict`.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpAgentEvent {
    /// The event kind.
    pub kind: AcpEventKind,
    /// The event payload (string or object).
    pub data: Value,
}

impl AcpAgentEvent {
    /// Build an event with the given kind and payload.
    pub fn new(kind: AcpEventKind, data: Value) -> Self {
        Self { kind, data }
    }
}

/// A failure starting an ACP run.
#[derive(Debug, Error)]
pub enum AcpRunError {
    /// The requested `cwd` is not an existing local directory.
    #[error("ACP agent workspace must be an existing local directory")]
    Workspace,
    /// Persisting the initial audit row failed.
    #[error(transparent)]
    Audit(#[from] AcpAuditError),
}

/// Everything needed to start one ACP turn.
pub struct AcpRunRequest {
    /// Owning user id.
    pub owner_id: String,
    /// Group id, if part of a group turn.
    pub group_id: Option<String>,
    /// The agent being run.
    pub agent_id: String,
    /// Thread id, if bound to a thread.
    pub thread_id: Option<String>,
    /// The normalized runtime config.
    pub config: AcpRuntimeConfig,
    /// The working directory the agent runs in (must exist).
    pub cwd: PathBuf,
    /// The user prompt text for this turn.
    pub prompt: String,
}

/// A cancellation handle for an in-flight ACP run.
///
/// Cloning shares the same cancellation signal. [`cancel`](AcpRunControl::cancel)
/// requests prompt termination: the child is killed and the run is persisted
/// `cancelled`.
#[derive(Clone)]
pub struct AcpRunControl {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl AcpRunControl {
    /// Request cancellation of the run.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
}

/// A started ACP run: a stream of [`AcpAgentEvent`]s plus a cancellation
/// [`AcpRunControl`] and the audit `run_id`.
///
/// Drain [`next_event`](AcpRun::next_event) until it returns `None`; by then the
/// terminal audit row has been persisted.
pub struct AcpRun {
    run_id: String,
    events: mpsc::UnboundedReceiver<AcpAgentEvent>,
    control: AcpRunControl,
    handle: JoinHandle<()>,
}

impl AcpRun {
    /// The `external_agent_runs.id` for this run.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// A cancellation handle for this run.
    pub fn control(&self) -> AcpRunControl {
        self.control.clone()
    }

    /// Receive the next streamed event, or `None` once the run has finished and
    /// its terminal audit row is persisted.
    pub async fn next_event(&mut self) -> Option<AcpAgentEvent> {
        self.events.recv().await
    }

    /// Await the background driver task to completion.
    pub async fn join(self) -> Result<(), tokio::task::JoinError> {
        self.handle.await
    }
}

/// Start an ACP agent turn and return a streaming [`AcpRun`].
///
/// Inserts the `running` audit row and emits the initial `run` event before
/// returning, then spawns a background task that drives the session and
/// persists the terminal state. The returned [`AcpRun`] yields events until the
/// run finishes.
pub async fn run_acp_agent_stream(
    pool: SqlitePool,
    request: AcpRunRequest,
) -> Result<AcpRun, AcpRunError> {
    if !request.cwd.is_dir() {
        return Err(AcpRunError::Workspace);
    }
    let cwd = request
        .cwd
        .canonicalize()
        .map_err(|_| AcpRunError::Workspace)?;
    let cwd_display = cwd.to_string_lossy().to_string();

    let mut argv = Vec::with_capacity(request.config.args.len() + 1);
    argv.push(request.config.command.clone());
    argv.extend(request.config.args.iter().cloned());

    let ctx = AcpRunContext {
        owner_id: request.owner_id.clone(),
        group_id: request.group_id.clone(),
        agent_id: request.agent_id.clone(),
        thread_id: request.thread_id.clone(),
        cwd: cwd_display.clone(),
        argv,
    };
    let audit = AcpRunAudit::start(&pool, &ctx).await?;
    let run_id = audit.id().to_string();

    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let _ = events_tx.send(run_event(
        &run_id,
        &request.agent_id,
        &cwd_display,
        "running",
        None,
        None,
    ));

    let control = AcpRunControl {
        cancelled: Arc::new(AtomicBool::new(false)),
        notify: Arc::new(Notify::new()),
    };

    let task = DriveTask {
        audit,
        run_id: run_id.clone(),
        agent_id: request.agent_id,
        config: request.config,
        cwd,
        cwd_display,
        prompt: request.prompt,
        events_tx,
        cancelled: control.cancelled.clone(),
        notify: control.notify.clone(),
    };
    let handle = tokio::spawn(drive_run(task));

    Ok(AcpRun {
        run_id,
        events: events_rx,
        control,
        handle,
    })
}

/// The terminal status of one ACP turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnStatus {
    Completed,
    Failed,
    Timeout,
    Cancelled,
}

/// The result of driving one ACP turn.
struct TurnOutcome {
    status: TurnStatus,
    error_message: Option<String>,
    exit_code: Option<i64>,
    stdout_tail: Option<String>,
    stderr_tail: Option<String>,
    /// True only for caller-initiated cancellation; suppresses the terminal
    /// `run` event (matching the Python runtime), but the audit row is still
    /// persisted as `cancelled`.
    was_cancelled: bool,
}

/// All state the background driver task owns.
struct DriveTask {
    audit: AcpRunAudit,
    run_id: String,
    agent_id: String,
    config: AcpRuntimeConfig,
    cwd: PathBuf,
    cwd_display: String,
    prompt: String,
    events_tx: mpsc::UnboundedSender<AcpAgentEvent>,
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

/// Drive one turn to completion and persist its terminal audit state.
async fn drive_run(task: DriveTask) {
    let DriveTask {
        audit,
        run_id,
        agent_id,
        config,
        cwd,
        cwd_display,
        prompt,
        events_tx,
        cancelled,
        notify,
    } = task;

    let outcome = run_turn(&config, &cwd, &prompt, &events_tx, &cancelled, &notify).await;

    let stdout_tail = outcome.stdout_tail.as_deref();
    let stderr_tail = outcome.stderr_tail.as_deref();
    let error_message = outcome
        .error_message
        .clone()
        .unwrap_or_else(|| "ACP agent failed".to_string());

    let persisted = match outcome.status {
        TurnStatus::Completed => {
            audit
                .complete(outcome.exit_code, stdout_tail, stderr_tail)
                .await
        }
        TurnStatus::Failed | TurnStatus::Timeout => {
            audit
                .fail(outcome.exit_code, stdout_tail, stderr_tail, &error_message)
                .await
        }
        TurnStatus::Cancelled => audit.cancel(stdout_tail, stderr_tail, &error_message).await,
    };
    if let Err(err) = persisted {
        tracing::error!(run_id = %run_id, error = %err, "failed to persist ACP audit row");
    }

    if !outcome.was_cancelled {
        let status = match outcome.status {
            TurnStatus::Completed => "completed",
            // The audit row stores timeouts as `failed`; report the same status
            // on the wire, with the timeout reason carried in `summary`.
            TurnStatus::Failed | TurnStatus::Timeout => "failed",
            TurnStatus::Cancelled => "cancelled",
        };
        let _ = events_tx.send(run_event(
            &run_id,
            &agent_id,
            &cwd_display,
            status,
            outcome.exit_code,
            outcome.error_message.as_deref(),
        ));
    }
    // Dropping `events_tx` here closes the stream once all queued events drain.
}

/// Spawn the child, drive the ACP session under a timeout and cancellation, and
/// return the turn outcome.
async fn run_turn(
    config: &AcpRuntimeConfig,
    cwd: &Path,
    prompt: &str,
    events_tx: &mpsc::UnboundedSender<AcpAgentEvent>,
    cancelled: &Arc<AtomicBool>,
    notify: &Arc<Notify>,
) -> TurnOutcome {
    let home = match tempfile::Builder::new().prefix("ag-swarmer-acp-").tempdir() {
        Ok(dir) => dir,
        Err(err) => return failed_outcome(format!("failed to create ACP home: {err}")),
    };
    let env = match build_child_env(config.profile, home.path(), &config.env) {
        Ok(env) => env,
        Err(err) => return failed_outcome(format!("failed to build ACP environment: {err}")),
    };

    let spawned = match spawn_acp_child(&config.command, &config.args, cwd, &env) {
        Ok(spawned) => spawned,
        Err(err) => return failed_outcome(format!("failed to start ACP agent: {err}")),
    };
    let SpawnedAcpChild {
        mut child,
        stdin,
        stdout,
        stderr,
    } = spawned;

    // Capture stderr into a bounded tail concurrently so a chatty agent cannot
    // deadlock on a full pipe.
    let stderr_task: JoinHandle<String> = tokio::spawn(async move {
        let mut tail = Tail::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tail.append(&line);
            tail.append("\n");
        }
        tail.into_string()
    });

    let conn = AcpConnection::spawn(stdin, stdout, config.permission_policy, events_tx.clone());

    let cwd_string = cwd.to_string_lossy().to_string();
    let phase = {
        let cancel_fut = wait_for_cancel(cancelled, notify);
        let session = drive_session(&conn, &cwd_string, prompt, config);
        let session = timeout(Duration::from_secs(config.timeout_seconds as u64), session);
        tokio::pin!(session);
        tokio::select! {
            biased;
            _ = cancel_fut => Phase::Cancelled,
            result = &mut session => match result {
                Ok(inner) => Phase::Done(inner),
                Err(_elapsed) => Phase::TimedOut,
            },
        }
    };

    let (status, error_message, was_cancelled, completed_cleanly) = match phase {
        Phase::Done(Ok(prompt_outcome)) => {
            if prompt_outcome.stop_reason == "cancelled" {
                (
                    TurnStatus::Cancelled,
                    Some("ACP agent cancelled the turn".to_string()),
                    false,
                    true,
                )
            } else {
                (TurnStatus::Completed, None, false, true)
            }
        }
        Phase::Done(Err(err)) => (TurnStatus::Failed, Some(err.to_string()), false, false),
        Phase::TimedOut => {
            conn.notify(METHOD_SESSION_CANCEL, json!({}));
            (
                TurnStatus::Timeout,
                Some(format!(
                    "ACP agent timed out after {} seconds",
                    config.timeout_seconds
                )),
                false,
                false,
            )
        }
        Phase::Cancelled => {
            conn.notify(METHOD_SESSION_CANCEL, json!({}));
            (
                TurnStatus::Cancelled,
                Some("ACP agent run was cancelled".to_string()),
                true,
                false,
            )
        }
    };

    let exit_code = if completed_cleanly && status == TurnStatus::Completed {
        // Close stdin so a child looping on stdin sees EOF and exits on its
        // own, then collect its exit code while stdout continues to drain.
        conn.close_stdin();
        wait_for_exit(&mut child).await
    } else {
        let _ = child.start_kill();
        let _ = timeout(CHILD_EXIT_GRACE, child.wait()).await;
        None
    };
    let stdout_tail = conn.shutdown(STDOUT_DRAIN_GRACE).await;

    let stderr_tail = match timeout(STDERR_DRAIN_GRACE, stderr_task).await {
        Ok(Ok(text)) if !text.is_empty() => Some(text),
        _ => None,
    };
    // Keep the isolated home alive until the child has exited.
    drop(home);

    TurnOutcome {
        status,
        error_message,
        exit_code,
        stdout_tail,
        stderr_tail,
        was_cancelled,
    }
}

/// The branch the turn took out of the timeout/cancel select.
enum Phase {
    Done(Result<PromptOutcome, ProtocolError>),
    TimedOut,
    Cancelled,
}

/// The result of a completed `session/prompt`.
struct PromptOutcome {
    stop_reason: String,
}

/// Resolve once cancellation has been requested. Uses the documented
/// register-then-check ordering so a `cancel()` racing with this future is not
/// missed.
async fn wait_for_cancel(cancelled: &Arc<AtomicBool>, notify: &Arc<Notify>) {
    loop {
        let notified = notify.notified();
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        notified.await;
        if cancelled.load(Ordering::Acquire) {
            return;
        }
    }
}

/// Run the ACP request sequence: `initialize` → `session/new` → session
/// settings → `session/prompt`, returning the prompt outcome. `session/update`
/// notifications are mapped to events by the connection's reader task while
/// this awaits the prompt response.
async fn drive_session(
    conn: &AcpConnection,
    cwd: &str,
    prompt: &str,
    config: &AcpRuntimeConfig,
) -> Result<PromptOutcome, ProtocolError> {
    conn.request(
        METHOD_INITIALIZE,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {},
            "clientInfo": { "name": "ag-swarmer", "title": "AG Swarmer", "version": "0.1.0" },
        }),
    )
    .await?;

    let mut new_params = json!({ "cwd": cwd, "mcpServers": [] });
    if let Some(meta) = new_session_meta(config) {
        new_params["_meta"] = meta;
    }
    let session = conn.request(METHOD_SESSION_NEW, new_params).await?;
    let session_id = session
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::Malformed("session/new returned no sessionId".to_string()))?
        .to_string();

    apply_session_settings(conn, &session_id, config).await?;

    let response = conn
        .request(
            METHOD_SESSION_PROMPT,
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": prompt }],
                "messageId": Uuid::new_v4().to_string(),
            }),
        )
        .await?;
    let stop_reason = response
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(PromptOutcome { stop_reason })
}

/// Build the `session/new` `_meta`, mirroring Python `_new_session_meta`: for
/// the `claude` profile with a thinking effort, attach `claudeCode.options`.
fn new_session_meta(config: &AcpRuntimeConfig) -> Option<Value> {
    if config.profile != AcpRuntimeProfile::Claude {
        return None;
    }
    let effort = config.thinking_effort.as_deref()?;
    Some(json!({ "claudeCode": { "options": { "effortLevel": effort } } }))
}

/// Apply model, mode, thinking effort, and explicit config options to a new
/// session, mirroring Python `_apply_session_settings`.
async fn apply_session_settings(
    conn: &AcpConnection,
    session_id: &str,
    config: &AcpRuntimeConfig,
) -> Result<(), ProtocolError> {
    if let Some(model) = config.model.as_deref() {
        apply_session_model(conn, session_id, model).await?;
    }
    if let Some(mode) = config.mode.as_deref() {
        apply_session_mode(conn, session_id, mode, config.profile).await?;
    }
    if let Some(effort) = config.thinking_effort.as_deref() {
        apply_first_config_option(
            conn,
            session_id,
            &thinking_config_option_ids(config.profile),
            &Value::String(effort.to_string()),
        )
        .await?;
    }
    if let Some(options) = &config.config_options {
        for (key, value) in options {
            let value = match value {
                AcpConfigValue::Str(s) => Value::String(s.clone()),
                AcpConfigValue::Bool(b) => Value::Bool(*b),
            };
            conn.request(
                METHOD_SESSION_SET_CONFIG_OPTION,
                config_option_params(key, session_id, &value),
            )
            .await?;
        }
    }
    Ok(())
}

/// Set the session model, falling back to a `model` config option only when the
/// agent does not implement `session/set_model`.
async fn apply_session_model(
    conn: &AcpConnection,
    session_id: &str,
    model: &str,
) -> Result<(), ProtocolError> {
    match conn
        .request(
            METHOD_SESSION_SET_MODEL,
            json!({ "modelId": model, "sessionId": session_id }),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(err) if err.is_method_not_found() => {
            apply_first_config_option(
                conn,
                session_id,
                &["model"],
                &Value::String(model.to_string()),
            )
            .await
        }
        Err(err) => Err(err),
    }
}

/// Set the session mode, falling back to profile-aware config options only when
/// the agent does not implement `session/set_mode`.
async fn apply_session_mode(
    conn: &AcpConnection,
    session_id: &str,
    mode: &str,
    profile: AcpRuntimeProfile,
) -> Result<(), ProtocolError> {
    match conn
        .request(
            METHOD_SESSION_SET_MODE,
            json!({ "modeId": mode, "sessionId": session_id }),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(err) if err.is_method_not_found() => {
            let ids = mode_config_option_ids(profile);
            apply_first_config_option(conn, session_id, &ids, &Value::String(mode.to_string()))
                .await
        }
        Err(err) => Err(err),
    }
}

/// Try each config-option id in order, returning on the first success and
/// surfacing the last error if all fail. Mirrors Python
/// `_apply_first_config_option` (it retries on *any* error, not only
/// method-not-found).
async fn apply_first_config_option(
    conn: &AcpConnection,
    session_id: &str,
    option_ids: &[&str],
    value: &Value,
) -> Result<(), ProtocolError> {
    let mut last_error: Option<ProtocolError> = None;
    for option_id in option_ids {
        match conn
            .request(
                METHOD_SESSION_SET_CONFIG_OPTION,
                config_option_params(option_id, session_id, value),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }
    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Build `session/set_config_option` params. A boolean value carries the
/// `type: "boolean"` discriminator; a string value is a select option.
fn config_option_params(config_id: &str, session_id: &str, value: &Value) -> Value {
    if value.is_boolean() {
        json!({ "configId": config_id, "sessionId": session_id, "type": "boolean", "value": value })
    } else {
        json!({ "configId": config_id, "sessionId": session_id, "value": value })
    }
}

/// Profile-aware thinking-effort config-option ids, mirroring Python
/// `_thinking_config_option_ids`.
fn thinking_config_option_ids(profile: AcpRuntimeProfile) -> [&'static str; 3] {
    match profile {
        AcpRuntimeProfile::Claude => ["effort", "effortLevel", "reasoning_effort"],
        _ => ["reasoning_effort", "effort", "effortLevel"],
    }
}

/// Profile-aware mode config-option ids, mirroring Python
/// `_mode_config_option_ids`.
fn mode_config_option_ids(profile: AcpRuntimeProfile) -> Vec<&'static str> {
    match profile {
        AcpRuntimeProfile::Claude => vec!["mode", "permissionMode", "permissions.defaultMode"],
        _ => vec!["mode", "approval_preset"],
    }
}

/// Build a `run` lifecycle event, mirroring Python `_run_event`.
fn run_event(
    run_id: &str,
    agent_id: &str,
    cwd: &str,
    status: &str,
    exit_code: Option<i64>,
    summary: Option<&str>,
) -> AcpAgentEvent {
    let mut payload = json!({
        "run_id": run_id,
        "agent_id": agent_id,
        "adapter": "acp",
        "status": status,
        "cwd": cwd,
    });
    if let Some(code) = exit_code {
        payload["exit_code"] = json!(code);
    }
    if let Some(summary) = summary {
        payload["summary"] = json!(summary);
    }
    AcpAgentEvent::new(AcpEventKind::Run, payload)
}

/// Wait for a child to exit within the grace period, returning its exit code.
async fn wait_for_exit(child: &mut Child) -> Option<i64> {
    match timeout(CHILD_EXIT_GRACE, child.wait()).await {
        Ok(Ok(status)) => status.code().map(|code| code as i64),
        _ => {
            let _ = child.start_kill();
            None
        }
    }
}

/// Build a `failed` outcome with the given message and no child output.
fn failed_outcome(message: String) -> TurnOutcome {
    TurnOutcome {
        status: TurnStatus::Failed,
        error_message: Some(message),
        exit_code: None,
        stdout_tail: None,
        stderr_tail: None,
        was_cancelled: false,
    }
}
