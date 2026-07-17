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

pub mod capabilities;
pub mod config;
pub mod process;
pub mod protocol;

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};

use serde_json::{json, Value};
use sqlx::SqlitePool;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
    sync::{mpsc, Mutex, Notify},
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

pub use capabilities::{
    probe_acp_runtime_capabilities, AcpCapabilityChoice, AcpCapabilityError, AcpRuntimeCapabilities,
};
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

/// A terminal failure from an ACP run's background driver.
///
/// Protocol failures and timeouts retain their safe user-facing message while
/// cancellation remains a separate outcome. Driver task failures never expose
/// panic details.
#[derive(Debug, Error)]
pub enum AcpRunJoinError {
    /// The ACP peer rejected or otherwise failed the run.
    #[error("{0}")]
    Failed(String),
    /// The configured ACP turn timeout elapsed.
    #[error("{0}")]
    Timeout(String),
    /// The run was cancelled by the caller or ACP peer.
    #[error("{0}")]
    Cancelled(String),
    /// The background driver task terminated unexpectedly.
    #[error("ACP run task failed")]
    Driver(#[source] tokio::task::JoinError),
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
    /// Incremental prompt to use when a matching live ACP session is reused.
    ///
    /// If no reusable session is available this is ignored and `prompt` is sent
    /// as the first full-context prompt.
    pub incremental_prompt: Option<String>,
    /// Optional hash of host-side context that should invalidate a reusable ACP
    /// session when it changes.
    pub context_hash: Option<String>,
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
    handle: JoinHandle<Result<(), AcpRunJoinError>>,
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
    pub async fn join(self) -> Result<(), AcpRunJoinError> {
        self.handle.await.map_err(AcpRunJoinError::Driver)?
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
        owner_id: request.owner_id,
        group_id: request.group_id,
        agent_id: request.agent_id,
        thread_id: request.thread_id,
        config: request.config,
        cwd,
        cwd_display,
        prompt: request.prompt,
        incremental_prompt: request.incremental_prompt,
        context_hash: request.context_hash,
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
    owner_id: String,
    group_id: Option<String>,
    agent_id: String,
    thread_id: Option<String>,
    config: AcpRuntimeConfig,
    cwd: PathBuf,
    cwd_display: String,
    prompt: String,
    incremental_prompt: Option<String>,
    context_hash: Option<String>,
    events_tx: mpsc::UnboundedSender<AcpAgentEvent>,
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

/// Drive one turn to completion and persist its terminal audit state.
async fn drive_run(task: DriveTask) -> Result<(), AcpRunJoinError> {
    let DriveTask {
        audit,
        run_id,
        owner_id,
        group_id,
        agent_id,
        thread_id,
        config,
        cwd,
        cwd_display,
        prompt,
        incremental_prompt,
        context_hash,
        events_tx,
        cancelled,
        notify,
    } = task;

    let reuse_key = reusable_session_key(group_id.as_deref(), thread_id.as_deref(), &agent_id);
    let mut outcome = if let Some(key) = reuse_key {
        run_reusable_turn(ReusableTurn {
            key,
            owner_id,
            agent_id: agent_id.clone(),
            config: config.clone(),
            cwd: cwd.clone(),
            full_prompt: prompt,
            incremental_prompt,
            context_hash,
            events_tx: events_tx.clone(),
            cancelled: cancelled.clone(),
            notify: notify.clone(),
        })
        .await
    } else {
        run_one_shot_turn(&config, &cwd, &prompt, &events_tx, &cancelled, &notify).await
    };

    if outcome.status == TurnStatus::Failed {
        if let Some(message) = outcome.error_message.as_mut() {
            *message = sanitize_failure_message(message, &config);
        }
    }

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

    match outcome.status {
        TurnStatus::Completed => Ok(()),
        TurnStatus::Failed => Err(AcpRunJoinError::Failed(error_message)),
        TurnStatus::Timeout => Err(AcpRunJoinError::Timeout(error_message)),
        TurnStatus::Cancelled => Err(AcpRunJoinError::Cancelled(error_message)),
    }
}

/// Spawn the child, drive the ACP session under a timeout and cancellation, and
/// return the turn outcome.
async fn run_one_shot_turn(
    config: &AcpRuntimeConfig,
    cwd: &Path,
    prompt: &str,
    events_tx: &mpsc::UnboundedSender<AcpAgentEvent>,
    cancelled: &Arc<AtomicBool>,
    notify: &Arc<Notify>,
) -> TurnOutcome {
    let mut session = match LiveAcpSession::start(config, cwd, events_tx.clone()).await {
        Ok(session) => session,
        Err(message) => return failed_outcome(message),
    };
    let cwd_string = cwd.to_string_lossy().to_string();
    let phase = drive_new_session_prompt(
        session.conn(),
        &cwd_string,
        prompt,
        config,
        cancelled,
        notify,
    )
    .await;

    let (status, error_message, was_cancelled, completed_cleanly) = phase_status(phase, config);

    finish_session(&mut session, status, completed_cleanly)
        .await
        .into_outcome(status, error_message, was_cancelled)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReusableSessionKey {
    group_id: String,
    thread_id: String,
    agent_id: String,
}

fn reusable_session_key(
    group_id: Option<&str>,
    thread_id: Option<&str>,
    agent_id: &str,
) -> Option<ReusableSessionKey> {
    Some(ReusableSessionKey {
        group_id: group_id?.to_string(),
        thread_id: thread_id?.to_string(),
        agent_id: agent_id.to_string(),
    })
}

struct ReusableTurn {
    key: ReusableSessionKey,
    owner_id: String,
    agent_id: String,
    config: AcpRuntimeConfig,
    cwd: PathBuf,
    full_prompt: String,
    incremental_prompt: Option<String>,
    context_hash: Option<String>,
    events_tx: mpsc::UnboundedSender<AcpAgentEvent>,
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

struct SessionManager {
    sessions: Mutex<HashMap<ReusableSessionKey, Arc<Mutex<ManagedAcpSession>>>>,
}

impl SessionManager {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    async fn get_or_insert(&self, key: ReusableSessionKey) -> Arc<Mutex<ManagedAcpSession>> {
        let mut sessions = self.sessions.lock().await;
        sessions
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(ManagedAcpSession::empty())))
            .clone()
    }
}

fn session_manager() -> &'static SessionManager {
    static MANAGER: OnceLock<SessionManager> = OnceLock::new();
    MANAGER.get_or_init(SessionManager::new)
}

/// Terminate all reusable in-process ACP sessions.
///
/// This is primarily used by tests and by future application shutdown hooks;
/// ordinary failed/cancelled turns clear the live session from their slot.
pub async fn shutdown_reusable_acp_sessions() {
    let sessions = {
        let mut sessions = session_manager().sessions.lock().await;
        sessions
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>()
    };
    for session in sessions {
        let mut managed = session.lock().await;
        if let Some(mut live) = managed.session.take() {
            let _ = terminate_live_session(&mut live).await;
        }
        managed.initialized = false;
    }
}

struct ManagedAcpSession {
    session: Option<LiveAcpSession>,
    signature: Option<SessionSignature>,
    initialized: bool,
}

impl ManagedAcpSession {
    fn empty() -> Self {
        Self {
            session: None,
            signature: None,
            initialized: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionSignature {
    owner_id: String,
    agent_id: String,
    cwd: PathBuf,
    config_hash: u64,
    context_hash: Option<String>,
}

impl SessionSignature {
    fn new(
        owner_id: String,
        agent_id: String,
        cwd: PathBuf,
        config: &AcpRuntimeConfig,
        context_hash: Option<String>,
    ) -> Self {
        let mut hasher = DefaultHasher::new();
        config.hash(&mut hasher);
        Self {
            owner_id,
            agent_id,
            cwd,
            config_hash: hasher.finish(),
            context_hash,
        }
    }
}

async fn run_reusable_turn(turn: ReusableTurn) -> TurnOutcome {
    let manager = session_manager();
    let slot = manager.get_or_insert(turn.key.clone()).await;
    let mut managed = slot.lock().await;

    let signature = SessionSignature::new(
        turn.owner_id.clone(),
        turn.agent_id.clone(),
        turn.cwd.clone(),
        &turn.config,
        turn.context_hash.clone(),
    );
    if managed.signature.as_ref() != Some(&signature) {
        if let Some(mut old) = managed.session.take() {
            let _ = terminate_live_session(&mut old).await;
        }
        managed.initialized = false;
        managed.signature = Some(signature);
    }

    if managed.session.is_none() {
        match LiveAcpSession::start(&turn.config, &turn.cwd, turn.events_tx.clone()).await {
            Ok(session) => managed.session = Some(session),
            Err(message) => {
                managed.initialized = false;
                return failed_outcome(message);
            }
        }
    } else if let Some(session) = managed.session.as_ref() {
        session.conn().set_events_tx(turn.events_tx.clone()).await;
    }

    let was_initialized = managed.initialized;
    let cwd_string = turn.cwd.to_string_lossy().to_string();
    let prompt = if was_initialized {
        turn.incremental_prompt
            .as_deref()
            .unwrap_or(&turn.full_prompt)
    } else {
        turn.full_prompt.as_str()
    };
    let phase = {
        let session = managed.session.as_mut().expect("managed session present");
        if was_initialized {
            drive_existing_session_prompt(
                session.conn(),
                &session.session_id,
                prompt,
                &turn.config,
                &turn.cancelled,
                &turn.notify,
            )
            .await
        } else {
            drive_new_session_prompt(
                session.conn(),
                &cwd_string,
                prompt,
                &turn.config,
                &turn.cancelled,
                &turn.notify,
            )
            .await
        }
    };
    if !was_initialized {
        if let Phase::Done(Ok(outcome)) = &phase {
            if outcome.stop_reason != "cancelled" {
                if let Some(session_id) = &outcome.session_id {
                    if let Some(session) = managed.session.as_mut() {
                        session.session_id = session_id.clone();
                    }
                }
                managed.initialized = true;
            }
        }
    }

    let (status, error_message, was_cancelled, completed_cleanly) =
        phase_status(phase, &turn.config);
    if status == TurnStatus::Completed && completed_cleanly {
        let conn = managed
            .session
            .as_ref()
            .expect("managed session present")
            .conn();
        let stdout_tail = conn.take_stdout_tail().await;
        conn.clear_events_tx().await;
        return TurnOutcome {
            status,
            error_message,
            exit_code: None,
            stdout_tail,
            stderr_tail: None,
            was_cancelled,
        };
    }

    let mut dead_session = managed.session.take();
    managed.initialized = false;
    drop(managed);
    if let Some(mut session) = dead_session.take() {
        return finish_session(&mut session, status, completed_cleanly)
            .await
            .into_outcome(status, error_message, was_cancelled);
    }
    failed_outcome(error_message.unwrap_or_else(|| "ACP session unavailable".to_string()))
}

struct LiveAcpSession {
    child: Child,
    conn: Option<AcpConnection>,
    stderr_task: Option<JoinHandle<String>>,
    home: Option<tempfile::TempDir>,
    session_id: String,
}

impl LiveAcpSession {
    async fn start(
        config: &AcpRuntimeConfig,
        cwd: &Path,
        events_tx: mpsc::UnboundedSender<AcpAgentEvent>,
    ) -> Result<Self, String> {
        let home = tempfile::Builder::new()
            .prefix("ag-swarmer-acp-")
            .tempdir()
            .map_err(|err| format!("failed to create ACP home: {err}"))?;
        let env = build_child_env(config.profile, home.path(), &config.env)
            .map_err(|err| format!("failed to build ACP environment: {err}"))?;
        let spawned = spawn_acp_child(&config.command, &config.args, cwd, &env)
            .map_err(|err| format!("failed to start ACP agent: {err}"))?;
        let SpawnedAcpChild {
            child,
            stdin,
            stdout,
            stderr,
        } = spawned;

        let stderr_task: JoinHandle<String> = tokio::spawn(async move {
            let mut tail = Tail::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tail.append(&line);
                tail.append("\n");
            }
            tail.into_string()
        });

        let conn = AcpConnection::spawn(stdin, stdout, config.permission_policy, events_tx, None);
        Ok(Self {
            child,
            conn: Some(conn),
            stderr_task: Some(stderr_task),
            home: Some(home),
            session_id: String::new(),
        })
    }

    fn conn(&self) -> &AcpConnection {
        self.conn.as_ref().expect("live ACP connection present")
    }

    fn take_conn(&mut self) -> AcpConnection {
        self.conn.take().expect("live ACP connection present")
    }

    fn take_stderr_task(&mut self) -> JoinHandle<String> {
        self.stderr_task.take().expect("stderr task present")
    }

    fn drop_home(&mut self) {
        drop(self.home.take());
    }
}

async fn terminate_live_session(session: &mut LiveAcpSession) -> SessionFinish {
    finish_session(session, TurnStatus::Cancelled, false).await
}

/// The branch the turn took out of the timeout/cancel select.
enum Phase {
    Done(Result<PromptOutcome, ProtocolError>),
    TimedOut,
    Cancelled,
}

fn phase_status(
    phase: Phase,
    config: &AcpRuntimeConfig,
) -> (TurnStatus, Option<String>, bool, bool) {
    match phase {
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
        Phase::TimedOut => (
            TurnStatus::Timeout,
            Some(format!(
                "ACP agent timed out after {} seconds",
                config.timeout_seconds
            )),
            false,
            false,
        ),
        Phase::Cancelled => (
            TurnStatus::Cancelled,
            Some("ACP agent run was cancelled".to_string()),
            true,
            false,
        ),
    }
}

struct SessionFinish {
    exit_code: Option<i64>,
    stdout_tail: Option<String>,
    stderr_tail: Option<String>,
}

impl SessionFinish {
    fn into_outcome(
        self,
        status: TurnStatus,
        error_message: Option<String>,
        was_cancelled: bool,
    ) -> TurnOutcome {
        TurnOutcome {
            status,
            error_message,
            exit_code: self.exit_code,
            stdout_tail: self.stdout_tail,
            stderr_tail: self.stderr_tail,
            was_cancelled,
        }
    }
}

async fn finish_session(
    session: &mut LiveAcpSession,
    status: TurnStatus,
    completed_cleanly: bool,
) -> SessionFinish {
    if matches!(status, TurnStatus::Timeout | TurnStatus::Cancelled) {
        session.conn().notify(METHOD_SESSION_CANCEL, json!({}));
    }

    let exit_code = if completed_cleanly && status == TurnStatus::Completed {
        // Close stdin so a child looping on stdin sees EOF and exits on its
        // own, then collect its exit code while stdout continues to drain.
        session.conn().close_stdin();
        wait_for_exit(&mut session.child).await
    } else if status == TurnStatus::Failed {
        wait_for_exit(&mut session.child).await
    } else {
        let _ = session.child.start_kill();
        let _ = timeout(CHILD_EXIT_GRACE, session.child.wait()).await;
        None
    };
    let stdout_tail = session.take_conn().shutdown(STDOUT_DRAIN_GRACE).await;

    let stderr_task = session.take_stderr_task();
    let stderr_tail = match timeout(STDERR_DRAIN_GRACE, stderr_task).await {
        Ok(Ok(text)) if !text.is_empty() => Some(text),
        _ => None,
    };
    // Keep the isolated home alive until the child has exited.
    session.drop_home();

    SessionFinish {
        exit_code,
        stdout_tail,
        stderr_tail,
    }
}

/// The result of a completed `session/prompt`.
struct PromptOutcome {
    stop_reason: String,
    session_id: Option<String>,
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
async fn new_session_prompt(
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
    Ok(PromptOutcome {
        stop_reason,
        session_id: Some(session_id),
    })
}

async fn drive_existing_session_prompt(
    conn: &AcpConnection,
    session_id: &str,
    prompt: &str,
    config: &AcpRuntimeConfig,
    cancelled: &Arc<AtomicBool>,
    notify: &Arc<Notify>,
) -> Phase {
    let cancel_fut = wait_for_cancel(cancelled, notify);
    let session = prompt_existing_session(conn, session_id, prompt);
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
}

async fn drive_new_session_prompt(
    conn: &AcpConnection,
    cwd: &str,
    prompt: &str,
    config: &AcpRuntimeConfig,
    cancelled: &Arc<AtomicBool>,
    notify: &Arc<Notify>,
) -> Phase {
    let cancel_fut = wait_for_cancel(cancelled, notify);
    let session = new_session_prompt(conn, cwd, prompt, config);
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
}

async fn prompt_existing_session(
    conn: &AcpConnection,
    session_id: &str,
    prompt: &str,
) -> Result<PromptOutcome, ProtocolError> {
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
    Ok(PromptOutcome {
        stop_reason,
        session_id: None,
    })
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

/// Remove configured environment values and unsafe formatting from a message
/// before it reaches audit summaries, stream events, or callers of `join`.
fn sanitize_failure_message(message: &str, config: &AcpRuntimeConfig) -> String {
    const MAX_CHARS: usize = 500;

    let mut sensitive_values = config
        .env
        .values()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    sensitive_values.sort_by_key(|value| std::cmp::Reverse(value.len()));

    let mut sanitized = message.to_string();
    for value in sensitive_values {
        sanitized = sanitized.replace(value, "[REDACTED]");
    }

    sanitized = sanitized
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();

    let compact = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "ACP agent failed".to_string();
    }

    let mut characters = compact.chars();
    let truncated = characters.by_ref().take(MAX_CHARS).collect::<String>();
    if characters.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
