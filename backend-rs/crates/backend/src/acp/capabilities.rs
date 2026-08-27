//! Short-lived ACP runtime capability discovery.
//!
//! A probe creates an isolated, non-reusable ACP process and performs only the
//! session setup and optional setting calls needed to observe model, mode, and
//! thinking selectors. It never sends `session/prompt`.

use std::{collections::HashSet, io, path::PathBuf, time::Duration};

use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
    sync::mpsc,
    task::JoinHandle,
    time::{timeout, Instant},
};

use super::{
    process::{build_probe_child_env, spawn_acp_child, SpawnedAcpChild, Tail},
    protocol::{
        AcpConnection, ProtocolError, METHOD_INITIALIZE, METHOD_SESSION_NEW,
        METHOD_SESSION_SET_CONFIG_OPTION, METHOD_SESSION_SET_MODEL, PROTOCOL_VERSION,
    },
    AcpAgentEvent, AcpRuntimeConfig, AcpRuntimeProfile, STDERR_DRAIN_GRACE, STDOUT_DRAIN_GRACE,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const CLEANUP_RESERVE: Duration = Duration::from_secs(1);
const UPDATE_QUIET_PERIOD: Duration = Duration::from_millis(100);
const CLEAN_EXIT_GRACE: Duration = Duration::from_millis(250);
const RAW_OBSERVATION_CAPACITY: usize = 64;

/// A normalized runtime-select choice advertised by an ACP adapter.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AcpCapabilityChoice {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

/// Model-related capabilities collected from one short-lived ACP session.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AcpRuntimeCapabilities {
    pub models: Vec<AcpCapabilityChoice>,
    pub modes: Vec<AcpCapabilityChoice>,
    pub thinking_efforts: Vec<AcpCapabilityChoice>,
    pub current_model: Option<String>,
    pub current_mode: Option<String>,
    pub current_thinking_effort: Option<String>,
    pub source: &'static str,
    pub warning: Option<String>,
}

impl AcpRuntimeCapabilities {
    /// Build an empty normalized response carrying a safe discovery warning.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            models: Vec::new(),
            modes: Vec::new(),
            thinking_efforts: Vec::new(),
            current_model: None,
            current_mode: None,
            current_thinking_effort: None,
            source: "acp",
            warning: Some(message.into()),
        }
    }
}

/// A safe, categorized ACP capability-probe failure.
#[derive(Debug, Error)]
pub enum AcpCapabilityError {
    #[error("Unable to prepare the ACP runtime capability probe.")]
    Environment {
        #[source]
        source: io::Error,
    },
    #[error("Unable to start the configured ACP runtime.")]
    Spawn {
        #[source]
        source: io::Error,
    },
    #[error("The configured ACP runtime rejected capability discovery.")]
    Protocol {
        #[source]
        source: ProtocolError,
    },
    #[error("ACP runtime capability discovery timed out after 15 seconds.")]
    Timeout,
}

impl AcpCapabilityError {
    fn protocol(source: ProtocolError) -> Self {
        Self::Protocol { source }
    }
}

/// Probe a configured ACP runtime without creating a reusable session or
/// sending a prompt.
pub async fn probe_acp_runtime_capabilities(
    config: AcpRuntimeConfig,
    selected_model: Option<String>,
) -> Result<AcpRuntimeCapabilities, AcpCapabilityError> {
    // A runtime with an unmet engine requirement fails deep inside its own
    // startup, where the only signal reaching us is "the child died". Say what
    // is actually wrong before paying for a spawn.
    if let Some(reason) = crate::acp::dsh::preflight(&config).await {
        return Ok(AcpRuntimeCapabilities::warning(reason));
    }
    let started = Instant::now();
    let deadline = started + PROBE_TIMEOUT;
    let (mut session, mut raw_updates_rx) = ProbeSession::start(&config).await?;
    let remaining = PROBE_TIMEOUT
        .saturating_sub(CLEANUP_RESERVE)
        .saturating_sub(started.elapsed());

    let result = if remaining.is_zero() {
        Err(AcpCapabilityError::Timeout)
    } else {
        match timeout(
            remaining,
            run_probe(&session, selected_model, &mut raw_updates_rx),
        )
        .await
        {
            Ok(result) => result.map_err(AcpCapabilityError::protocol),
            Err(_) => Err(AcpCapabilityError::Timeout),
        }
    };

    session.shutdown(deadline).await;
    result
}

async fn run_probe(
    session: &ProbeSession,
    selected_model: Option<String>,
    raw_updates_rx: &mut mpsc::Receiver<Value>,
) -> Result<AcpRuntimeCapabilities, ProtocolError> {
    let mut state = CapabilityState::new(session.profile);
    request_observing(
        session.conn(),
        raw_updates_rx,
        &mut state,
        METHOD_INITIALIZE,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "qunica",
                "title": "Qunica",
                "version": "0.1.0"
            },
        }),
    )
    .await?;

    let cwd = session.cwd.to_string_lossy().to_string();
    let new_session = request_observing(
        session.conn(),
        raw_updates_rx,
        &mut state,
        METHOD_SESSION_NEW,
        json!({ "cwd": cwd, "mcpServers": [] }),
    )
    .await?;
    let session_id = new_session
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::Malformed("session/new returned no sessionId".to_string()))?;

    let model_revision_before_settings = state.model_revision;
    if let Some(model) = selected_model.as_deref() {
        apply_selected_model(
            session.conn(),
            raw_updates_rx,
            &mut state,
            session_id,
            model,
        )
        .await?;
    }
    drain_observations(raw_updates_rx, &mut state).await;

    if state.model_revision == model_revision_before_settings {
        if let Some(model) = selected_model {
            state.assume_current_model(model);
        }
    }

    Ok(state.finish(&session.sensitive_values))
}

async fn request_observing(
    conn: &AcpConnection,
    raw_updates_rx: &mut mpsc::Receiver<Value>,
    state: &mut CapabilityState,
    method: &str,
    params: Value,
) -> Result<Value, ProtocolError> {
    let request = conn.request(method, params);
    tokio::pin!(request);
    loop {
        tokio::select! {
            result = &mut request => {
                drain_available_observations(raw_updates_rx, state);
                return result;
            }
            update = raw_updates_rx.recv() => {
                match update {
                    Some(update) => state.observe(&update),
                    None => return request.await,
                }
            }
        }
    }
}

/// Nudge the agent into revealing its model catalog by selecting the model the
/// user already picked, falling back to a `model` config option when the agent
/// does not implement `session/set_model`.
///
/// An agent that implements *neither* method has no wire-level model selector
/// at all — dsh takes its model from the composition it launched with and
/// answers method-not-found for every `session/set_*`. That is a discovery
/// result, not a probe failure: treating it as one would replace the whole
/// capability card with "rejected capability discovery" even though
/// `initialize` and `session/new` both succeeded. The caller then falls back to
/// the preset's own choices, and [`CapabilityState::assume_current_model`]
/// keeps the user's pick selected.
async fn apply_selected_model(
    conn: &AcpConnection,
    raw_updates_rx: &mut mpsc::Receiver<Value>,
    state: &mut CapabilityState,
    session_id: &str,
    model: &str,
) -> Result<(), ProtocolError> {
    match request_observing(
        conn,
        raw_updates_rx,
        state,
        METHOD_SESSION_SET_MODEL,
        json!({ "modelId": model, "sessionId": session_id }),
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(error) if error.is_method_not_found() => {
            match request_observing(
                conn,
                raw_updates_rx,
                state,
                METHOD_SESSION_SET_CONFIG_OPTION,
                json!({ "configId": "model", "sessionId": session_id, "value": model }),
            )
            .await
            {
                Ok(_) => Ok(()),
                Err(error) if error.is_method_not_found() => Ok(()),
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn drain_available_observations(
    raw_updates_rx: &mut mpsc::Receiver<Value>,
    state: &mut CapabilityState,
) {
    while let Ok(value) = raw_updates_rx.try_recv() {
        state.observe(&value);
    }
}

async fn drain_observations(
    raw_updates_rx: &mut mpsc::Receiver<Value>,
    state: &mut CapabilityState,
) {
    while let Ok(Some(update)) = timeout(UPDATE_QUIET_PERIOD, raw_updates_rx.recv()).await {
        state.observe(&update);
    }
}

struct ProbeSession {
    child: Child,
    conn: Option<AcpConnection>,
    stderr_task: Option<JoinHandle<()>>,
    temp_root: Option<tempfile::TempDir>,
    cwd: PathBuf,
    sensitive_values: Vec<String>,
    profile: AcpRuntimeProfile,
}

impl ProbeSession {
    async fn start(
        config: &AcpRuntimeConfig,
    ) -> Result<(Self, mpsc::Receiver<Value>), AcpCapabilityError> {
        let temp_root = tempfile::Builder::new()
            .prefix("qunica-acp-probe-")
            .tempdir()
            .map_err(|source| AcpCapabilityError::Environment { source })?;
        let home = temp_root.path().join("home");
        let cwd = temp_root.path().join("work");
        std::fs::create_dir_all(&cwd)
            .map_err(|source| AcpCapabilityError::Environment { source })?;
        let probe_env = build_probe_child_env(config.profile, &home, &config.env)
            .map_err(|source| AcpCapabilityError::Environment { source })?;
        let args = crate::acp::dsh::launch_args(config, &home, &cwd)
            .map_err(|source| AcpCapabilityError::Environment { source })?;
        let SpawnedAcpChild {
            child,
            stdin,
            stdout,
            stderr,
        } = spawn_acp_child(&config.command, &args, &cwd, &probe_env.variables)
            .map_err(|source| AcpCapabilityError::Spawn { source })?;

        let stderr_task = tokio::spawn(async move {
            let mut tail = Tail::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tail.append(&line);
                tail.append("\n");
            }
        });
        let (events_tx, _events_rx) = mpsc::unbounded_channel::<AcpAgentEvent>();
        let (raw_updates_tx, raw_updates_rx) = mpsc::channel(RAW_OBSERVATION_CAPACITY);
        let conn = AcpConnection::spawn(
            stdin,
            stdout,
            config.permission_policy,
            events_tx,
            Some(raw_updates_tx),
        );

        Ok((
            Self {
                child,
                conn: Some(conn),
                stderr_task: Some(stderr_task),
                temp_root: Some(temp_root),
                cwd,
                sensitive_values: probe_env.sensitive_values,
                profile: config.profile,
            },
            raw_updates_rx,
        ))
    }

    fn conn(&self) -> &AcpConnection {
        self.conn.as_ref().expect("ACP probe connection present")
    }

    async fn shutdown(&mut self, deadline: Instant) {
        if let Some(conn) = self.conn.as_ref() {
            conn.close_stdin();
        }

        let clean_exit_grace = remaining_until(deadline).min(CLEAN_EXIT_GRACE);
        let exited = !clean_exit_grace.is_zero()
            && matches!(
                timeout(clean_exit_grace, self.child.wait()).await,
                Ok(Ok(_))
            );
        if !exited {
            let _ = self.child.start_kill();
            let kill_grace = remaining_until(deadline);
            if !kill_grace.is_zero() {
                let _ = timeout(kill_grace, self.child.wait()).await;
            }
            // `kill_on_drop` provides the last bounded kill attempt. Never
            // extend the public deadline for an OS process anomaly.
        }

        if let Some(conn) = self.conn.take() {
            let drain_grace = remaining_until(deadline).min(STDOUT_DRAIN_GRACE);
            let _ = conn.shutdown(drain_grace).await;
        }
        if let Some(mut stderr_task) = self.stderr_task.take() {
            let drain_grace = remaining_until(deadline).min(STDERR_DRAIN_GRACE);
            if drain_grace.is_zero() || timeout(drain_grace, &mut stderr_task).await.is_err() {
                stderr_task.abort();
            }
        }
        drop(self.temp_root.take());
    }
}

#[derive(Default)]
struct CapabilityState {
    profile: AcpRuntimeProfile,
    model: Option<SelectCapability>,
    mode: Option<SelectCapability>,
    thinking: Option<SelectCapability>,
    legacy_mode: Option<SelectCapability>,
    model_revision: usize,
    mode_revision: usize,
    legacy_mode_revision: usize,
    wire_revision: usize,
}

#[derive(Clone, Default)]
struct SelectCapability {
    choices: Vec<AcpCapabilityChoice>,
    current: Option<String>,
}

impl CapabilityState {
    fn new(profile: AcpRuntimeProfile) -> Self {
        Self {
            profile,
            ..Self::default()
        }
    }

    fn observe(&mut self, value: &Value) {
        self.wire_revision = self.wire_revision.saturating_add(1);
        let revision = self.wire_revision;
        if let Some(options) = value.get("configOptions").and_then(Value::as_array) {
            if let Some(model) = select_capability(options, "model", &["model"]) {
                self.model = Some(model);
                self.model_revision = self.model_revision.saturating_add(1);
            }
            if let Some(mode) = select_capability(options, "mode", &["mode", "approval_preset"]) {
                self.mode = Some(mode);
                self.mode_revision = revision;
            }
            if let Some(thinking) = select_capability(
                options,
                "thought_level",
                &["reasoning_effort", "effort", "effortLevel"],
            ) {
                self.thinking = Some(thinking);
            }
        }

        if let Some(models) = available_models(value) {
            if self.profile != AcpRuntimeProfile::Opencode || self.model.is_none() {
                let existing = self.model.take();
                let current = existing
                    .as_ref()
                    .and_then(|model| model.current.clone())
                    .or(models.current);
                self.model = Some(SelectCapability {
                    choices: merge_choices(models.choices, existing),
                    current,
                });
                self.model_revision = self.model_revision.saturating_add(1);
            }
        }

        if let Some(modes) = value.get("modes") {
            self.legacy_mode = legacy_modes(modes);
            if self.legacy_mode.is_some() {
                self.legacy_mode_revision = revision;
            }
        }
        if value.get("sessionUpdate").and_then(Value::as_str) == Some("current_mode_update") {
            if let Some(mode_id) = value
                .get("currentModeId")
                .and_then(nonempty_str)
                .or_else(|| value.get("modeId").and_then(nonempty_str))
            {
                let config_mode_is_newest = self.mode_revision >= self.legacy_mode_revision;
                let mode = if config_mode_is_newest {
                    self.mode.get_or_insert_with(Default::default)
                } else {
                    self.legacy_mode.get_or_insert_with(Default::default)
                };
                mode.current = Some(mode_id.to_string());
                if config_mode_is_newest {
                    self.mode_revision = revision;
                } else {
                    self.legacy_mode_revision = revision;
                }
            }
        }
    }

    fn assume_current_model(&mut self, model: String) {
        self.model.get_or_insert_with(Default::default).current = Some(model);
    }

    fn finish(self, sensitive_values: &[String]) -> AcpRuntimeCapabilities {
        let model = self.model.unwrap_or_default();
        let mode = if self.mode_revision >= self.legacy_mode_revision {
            self.mode.or(self.legacy_mode)
        } else {
            self.legacy_mode.or(self.mode)
        }
        .unwrap_or_default();
        let thinking = self.thinking.unwrap_or_default();
        let mut capabilities = AcpRuntimeCapabilities {
            models: model.choices,
            modes: mode.choices,
            thinking_efforts: thinking.choices,
            current_model: model.current,
            current_mode: mode.current,
            current_thinking_effort: thinking.current,
            source: "acp",
            warning: None,
        };
        capabilities.redact_sensitive_values(sensitive_values);
        capabilities
    }
}

fn available_models(value: &Value) -> Option<SelectCapability> {
    let models = value.get("models")?;
    let choices = models
        .get("availableModels")?
        .as_array()?
        .iter()
        .filter_map(|model| {
            let value = model.get("modelId").and_then(nonempty_str)?.to_string();
            let label = model
                .get("name")
                .and_then(nonempty_str)
                .unwrap_or(&value)
                .to_string();
            let description = model
                .get("description")
                .and_then(nonempty_str)
                .map(str::to_string);
            Some(AcpCapabilityChoice {
                value,
                label,
                description,
            })
        })
        .collect();
    Some(SelectCapability {
        choices,
        current: models
            .get("currentModelId")
            .and_then(nonempty_str)
            .map(str::to_string),
    })
}

fn merge_choices(
    mut primary: Vec<AcpCapabilityChoice>,
    secondary: Option<SelectCapability>,
) -> Vec<AcpCapabilityChoice> {
    let mut seen: HashSet<String> = primary.iter().map(|choice| choice.value.clone()).collect();
    if let Some(secondary) = secondary {
        for choice in secondary.choices {
            if seen.insert(choice.value.clone()) {
                primary.push(choice);
            }
        }
    }
    primary
}

impl AcpRuntimeCapabilities {
    fn redact_sensitive_values(&mut self, sensitive_values: &[String]) {
        redact_choices(&mut self.models, sensitive_values);
        redact_choices(&mut self.modes, sensitive_values);
        redact_choices(&mut self.thinking_efforts, sensitive_values);
        redact_current(&mut self.current_model, sensitive_values);
        redact_current(&mut self.current_mode, sensitive_values);
        redact_current(&mut self.current_thinking_effort, sensitive_values);
    }
}

fn redact_choices(choices: &mut Vec<AcpCapabilityChoice>, sensitive_values: &[String]) {
    choices.retain_mut(|choice| {
        if contains_sensitive(&choice.value, sensitive_values) {
            return false;
        }
        if contains_sensitive(&choice.label, sensitive_values) {
            choice.label.clone_from(&choice.value);
        }
        if choice
            .description
            .as_deref()
            .is_some_and(|value| contains_sensitive(value, sensitive_values))
        {
            choice.description = None;
        }
        true
    });
}

fn redact_current(current: &mut Option<String>, sensitive_values: &[String]) {
    if current
        .as_deref()
        .is_some_and(|value| contains_sensitive(value, sensitive_values))
    {
        *current = None;
    }
}

fn contains_sensitive(value: &str, sensitive_values: &[String]) -> bool {
    sensitive_values
        .iter()
        .any(|sensitive| !sensitive.is_empty() && value.contains(sensitive))
}

fn select_capability(
    config_options: &[Value],
    category: &str,
    aliases: &[&str],
) -> Option<SelectCapability> {
    let selected = config_options
        .iter()
        .find(|option| {
            is_select(option) && option.get("category").and_then(Value::as_str) == Some(category)
        })
        .or_else(|| {
            config_options.iter().find(|option| {
                is_select(option)
                    && option
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| aliases.contains(&id))
            })
        })?;

    Some(SelectCapability {
        choices: select_choices(selected),
        current: selected
            .get("currentValue")
            .and_then(nonempty_str)
            .map(str::to_string),
    })
}

fn is_select(option: &Value) -> bool {
    matches!(
        option.get("type").and_then(Value::as_str),
        None | Some("select")
    ) && option.get("options").is_some_and(Value::is_array)
}

fn select_choices(option: &Value) -> Vec<AcpCapabilityChoice> {
    let mut seen = HashSet::new();
    option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| {
            let value = choice.get("value").and_then(nonempty_str)?.to_string();
            if !seen.insert(value.clone()) {
                return None;
            }
            let label = choice
                .get("name")
                .and_then(nonempty_str)
                .or_else(|| choice.get("label").and_then(nonempty_str))
                .unwrap_or(&value)
                .to_string();
            let description = choice
                .get("description")
                .and_then(nonempty_str)
                .map(str::to_string);
            Some(AcpCapabilityChoice {
                value,
                label,
                description,
            })
        })
        .collect()
}

fn legacy_modes(value: &Value) -> Option<SelectCapability> {
    let modes = value.get("availableModes")?.as_array()?;
    let mut seen = HashSet::new();
    let choices = modes
        .iter()
        .filter_map(|mode| {
            let value = mode.get("id").and_then(nonempty_str)?.to_string();
            if !seen.insert(value.clone()) {
                return None;
            }
            let label = mode
                .get("name")
                .and_then(nonempty_str)
                .or_else(|| mode.get("label").and_then(nonempty_str))
                .unwrap_or(&value)
                .to_string();
            let description = mode
                .get("description")
                .and_then(nonempty_str)
                .map(str::to_string);
            Some(AcpCapabilityChoice {
                value,
                label,
                description,
            })
        })
        .collect();
    Some(SelectCapability {
        choices,
        current: value
            .get("currentModeId")
            .and_then(nonempty_str)
            .map(str::to_string),
    })
}

fn nonempty_str(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| !value.trim().is_empty())
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}
