//! ACP stdio JSON-RPC transport.
//!
//! # Wire framing
//!
//! ACP speaks **newline-delimited JSON-RPC 2.0** over the child's stdio: every
//! message is a single compact JSON object terminated by a `\n`, encoded UTF-8.
//! This matches the reference Python `acp` package, whose sender writes
//! `json.dumps(payload, separators=(",", ":")) + "\n"` and whose reader splits
//! incoming bytes on `\n` (`acp/task/sender.py`, `acp.transports`). There is no
//! `Content-Length` header (LSP framing is *not* used).
//!
//! Three message shapes travel over the wire:
//! - request:      `{"jsonrpc":"2.0","id":<n>,"method":"...","params":{...}}`
//! - response:     `{"jsonrpc":"2.0","id":<n>,"result":{...}}` or `{...,"error":{...}}`
//! - notification: `{"jsonrpc":"2.0","method":"...","params":{...}}` (no `id`)
//!
//! [`AcpConnection`] owns the stdio pipes through two background tasks: a writer
//! task that serializes outgoing lines, and a reader task that demultiplexes
//! incoming lines into (a) responses routed back to the matching pending
//! [`AcpConnection::request`], (b) `session/update` notifications mapped to
//! [`AcpAgentEvent`]s and sent to the currently active turn's event sink, and
//! (c) `session/request_permission` requests answered per the configured
//! [`PermissionPolicy`].
//!
//! The reader **skips any stdout line that is not a JSON object** (blank lines,
//! banner text, and, in tests, the integration harness's `running N tests`
//! header). Tolerating non-JSON lines here keeps a real agent's incidental
//! stdout chatter and the self-spawned fake-child test fixture from corrupting
//! the protocol stream.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};

use serde_json::{json, Value};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::{mpsc, oneshot, Mutex},
    task::JoinHandle,
    time::{timeout, Instant},
};

use crate::acp::config::PermissionPolicy;
use crate::acp::process::Tail;
use crate::acp::{AcpAgentEvent, AcpEventKind};

/// The ACP protocol version this client advertises in `initialize`.
pub const PROTOCOL_VERSION: i64 = 1;
/// The JSON-RPC version string used on every message.
pub const JSONRPC_VERSION: &str = "2.0";
/// JSON-RPC error code signalling the peer does not implement a method.
pub const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;

/// Largest tool input/output summary retained on a tool-call event.
pub const MAX_METADATA_CHARS: usize = 1_000;

/// `initialize` request method.
pub const METHOD_INITIALIZE: &str = "initialize";
/// `session/new` request method.
pub const METHOD_SESSION_NEW: &str = "session/new";
/// `session/set_model` request method.
pub const METHOD_SESSION_SET_MODEL: &str = "session/set_model";
/// `session/set_mode` request method.
pub const METHOD_SESSION_SET_MODE: &str = "session/set_mode";
/// `session/set_config_option` request method.
pub const METHOD_SESSION_SET_CONFIG_OPTION: &str = "session/set_config_option";
/// `session/prompt` request method.
pub const METHOD_SESSION_PROMPT: &str = "session/prompt";
/// `session/cancel` notification method.
pub const METHOD_SESSION_CANCEL: &str = "session/cancel";
/// `session/update` notification method (agent → client).
pub const METHOD_SESSION_UPDATE: &str = "session/update";
/// `session/request_permission` request method (agent → client).
pub const METHOD_SESSION_REQUEST_PERMISSION: &str = "session/request_permission";

/// A failure driving the ACP JSON-RPC protocol.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// The peer returned a JSON-RPC error object.
    #[error("ACP request failed ({code}): {message}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// JSON-RPC error message.
        message: String,
    },
    /// The transport closed before the request completed (child exited or the
    /// stdio pipe was dropped).
    #[error("ACP transport closed before the request completed")]
    TransportClosed,
    /// A response was missing an expected field.
    #[error("ACP response was malformed: {0}")]
    Malformed(String),
}

impl ProtocolError {
    /// Whether this is a JSON-RPC "method not found" error, which the session
    /// setup uses to decide whether to fall back to a config-option call.
    pub fn is_method_not_found(&self) -> bool {
        matches!(self, ProtocolError::Rpc { code, .. } if *code == JSONRPC_METHOD_NOT_FOUND)
    }
}

/// A JSON-RPC error returned over the wire.
#[derive(Debug, Clone)]
struct RpcError {
    code: i64,
    message: String,
}

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>>;
type EventSink = Arc<Mutex<Option<mpsc::UnboundedSender<AcpAgentEvent>>>>;
type RawUpdateSink = Option<mpsc::Sender<Value>>;

/// Messages sent to the background writer task.
enum WriterMessage {
    Line(String),
    Close,
}

/// A live ACP stdio JSON-RPC connection to a spawned agent child process.
pub struct AcpConnection {
    writer_tx: mpsc::UnboundedSender<WriterMessage>,
    pending: PendingMap,
    events_tx: EventSink,
    next_id: Arc<AtomicI64>,
    stdout_tail: Arc<Mutex<Tail>>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

impl AcpConnection {
    /// Take ownership of a child's stdio pipes and start the reader/writer
    /// tasks. `session/update` notifications are mapped to [`AcpAgentEvent`]s
    /// and forwarded on `events_tx`; permission requests are answered with
    /// `permission_policy`. When `raw_updates_tx` is present, response results
    /// and raw session updates are also forwarded in wire order for probes.
    pub fn spawn(
        stdin: ChildStdin,
        stdout: ChildStdout,
        permission_policy: PermissionPolicy,
        events_tx: mpsc::UnboundedSender<AcpAgentEvent>,
        raw_updates_tx: RawUpdateSink,
    ) -> Self {
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<WriterMessage>();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let events_tx: EventSink = Arc::new(Mutex::new(Some(events_tx)));
        let stdout_tail = Arc::new(Mutex::new(Tail::new()));

        let writer = tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(message) = writer_rx.recv().await {
                match message {
                    WriterMessage::Line(line) => {
                        if stdin.write_all(line.as_bytes()).await.is_err() {
                            break;
                        }
                        if stdin.flush().await.is_err() {
                            break;
                        }
                    }
                    WriterMessage::Close => break,
                }
            }
            // Dropping `stdin` here closes the child's stdin, signalling EOF.
        });

        let reader_pending = pending.clone();
        let reader_writer_tx = writer_tx.clone();
        let reader_events_tx = events_tx.clone();
        let reader_raw_updates_tx = raw_updates_tx;
        let reader_stdout_tail = stdout_tail.clone();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            // Ends on stdout EOF (`Ok(None)`) or a read error (`Err`).
            while let Ok(Some(line)) = lines.next_line().await {
                {
                    let mut tail = reader_stdout_tail.lock().await;
                    tail.append(&line);
                    tail.append("\n");
                }
                route_incoming(
                    &line,
                    &reader_pending,
                    &reader_writer_tx,
                    permission_policy,
                    &reader_events_tx,
                    &reader_raw_updates_tx,
                )
                .await;
            }
            // On EOF, drop every pending sender so in-flight requests resolve to
            // `TransportClosed` instead of hanging forever.
            reader_pending.lock().await.clear();
        });

        Self {
            writer_tx,
            pending,
            events_tx,
            next_id: Arc::new(AtomicI64::new(1)),
            stdout_tail,
            reader,
            writer,
        }
    }

    /// Replace the event sink used for future `session/update` notifications.
    ///
    /// Reusable ACP sessions keep one reader task across many prompt turns;
    /// each turn installs its own sink before calling `session/prompt` so
    /// streamed updates flow to that turn's [`AcpRun`].
    pub async fn set_events_tx(&self, events_tx: mpsc::UnboundedSender<AcpAgentEvent>) {
        *self.events_tx.lock().await = Some(events_tx);
    }

    /// Clear the current turn's event sink so that its receiver can close
    /// while a reusable ACP session stays alive for later prompts.
    pub async fn clear_events_tx(&self) {
        *self.events_tx.lock().await = None;
    }

    /// Push an event into the current turn's sink directly.
    ///
    /// Most events come from mapped `session/update` notifications; this is for
    /// notices the client itself raises about the session, such as a setting
    /// the agent does not implement. A turn with no installed sink drops it.
    pub async fn emit_event(&self, event: AcpAgentEvent) {
        if let Some(events_tx) = self.events_tx.lock().await.as_ref() {
            let _ = events_tx.send(event);
        }
    }

    /// Send a JSON-RPC request and await its response.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, ProtocolError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let line = encode_line(&json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "method": method,
            "params": params,
        }));
        if self.writer_tx.send(WriterMessage::Line(line)).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(ProtocolError::TransportClosed);
        }

        match rx.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => Err(ProtocolError::Rpc {
                code: err.code,
                message: err.message,
            }),
            Err(_) => Err(ProtocolError::TransportClosed),
        }
    }

    /// Send a fire-and-forget JSON-RPC notification (no response expected).
    pub fn notify(&self, method: &str, params: Value) {
        let _ = self.writer_tx.send(WriterMessage::Line(encode_line(&json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": method,
            "params": params,
        }))));
    }

    /// Ask the writer task to close the child's stdin while leaving stdout
    /// draining. Safe to call more than once.
    pub fn close_stdin(&self) {
        let _ = self.writer_tx.send(WriterMessage::Close);
    }

    /// The bounded stdout tail captured by the reader task so far.
    pub async fn stdout_tail(&self) -> Option<String> {
        let text = self.stdout_tail.lock().await.snapshot().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Take and clear the bounded stdout tail captured so far.
    pub async fn take_stdout_tail(&self) -> Option<String> {
        let text = std::mem::take(&mut *self.stdout_tail.lock().await).into_string();
        (!text.is_empty()).then_some(text)
    }

    /// Close the connection and return the bounded stdout tail.
    ///
    /// Call this after the child has exited (or after requesting a kill). The
    /// reader is allowed a short grace period to observe stdout EOF and drain
    /// the final lines; if the process refuses to die, the reader is aborted so
    /// shutdown cannot hang forever.
    pub async fn shutdown(mut self, reader_drain_grace: Duration) -> Option<String> {
        self.close_stdin();
        let stdout_tail = self.stdout_tail.clone();
        let deadline = Instant::now() + reader_drain_grace;
        if timeout(remaining_until(deadline), &mut self.reader)
            .await
            .is_err()
        {
            self.reader.abort();
        }
        if timeout(remaining_until(deadline), &mut self.writer)
            .await
            .is_err()
        {
            self.writer.abort();
        }
        let text = stdout_tail
            .try_lock()
            .map(|tail| tail.snapshot().to_string())
            .unwrap_or_default();
        (!text.is_empty()).then_some(text)
    }
}

/// Serialize a JSON value to a single compact line terminated by `\n`.
fn encode_line(value: &Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

/// Route one incoming stdout line to the right handler. Non-JSON-object lines
/// are skipped (see the module docs).
async fn route_incoming(
    line: &str,
    pending: &PendingMap,
    writer_tx: &mpsc::UnboundedSender<WriterMessage>,
    permission_policy: PermissionPolicy,
    events_tx: &EventSink,
    raw_updates_tx: &RawUpdateSink,
) {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return;
    }
    let message: Value = match serde_json::from_str(trimmed) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => return,
    };

    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id");

    match (method, id) {
        // Incoming request (agent → client): only permission is handled.
        (Some(method), Some(id)) if !id.is_null() => {
            let response =
                handle_incoming_request(method, id, message.get("params"), permission_policy);
            let _ = writer_tx.send(WriterMessage::Line(encode_line(&response)));
        }
        // Notification (no id).
        (Some(method), _) => {
            if method == METHOD_SESSION_UPDATE {
                if let Some(update) = message
                    .get("params")
                    .and_then(|params| params.get("update"))
                {
                    if let Some(raw_updates_tx) = raw_updates_tx {
                        let _ = raw_updates_tx.send(update.clone()).await;
                    }
                }
                if let Some(event) = message.get("params").and_then(event_from_update) {
                    let events_tx = events_tx.lock().await.clone();
                    if let Some(events_tx) = events_tx {
                        let _ = events_tx.send(event);
                    }
                }
            }
        }
        // Response to one of our requests.
        (None, Some(id)) => {
            if let Some(id) = id.as_i64() {
                let outcome = if let Some(error) = message.get("error") {
                    Err(RpcError {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                } else {
                    Ok(message.get("result").cloned().unwrap_or(Value::Null))
                };
                if let Some(tx) = pending.lock().await.remove(&id) {
                    if let (Ok(result), Some(raw_updates_tx)) = (&outcome, raw_updates_tx) {
                        let _ = raw_updates_tx.send(result.clone()).await;
                    }
                    let _ = tx.send(outcome);
                }
            }
        }
        (None, None) => {}
    }
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// Build the JSON-RPC response to an agent-initiated request. Only
/// `session/request_permission` is supported; anything else is rejected with
/// method-not-found, matching the Python client's stub handlers.
fn handle_incoming_request(
    method: &str,
    id: &Value,
    params: Option<&Value>,
    permission_policy: PermissionPolicy,
) -> Value {
    if method == METHOD_SESSION_REQUEST_PERMISSION {
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "result": { "outcome": decide_permission(params, permission_policy) },
        })
    } else {
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "error": {
                "code": JSONRPC_METHOD_NOT_FOUND,
                "message": format!("method not found: {method}"),
            },
        })
    }
}

/// Decide a permission request. `deny` always cancels; `auto_allow` selects the
/// first allow option, mirroring Python's `_first_allow_option`.
fn decide_permission(params: Option<&Value>, permission_policy: PermissionPolicy) -> Value {
    if permission_policy == PermissionPolicy::AutoAllow {
        if let Some(options) = params
            .and_then(|p| p.get("options"))
            .and_then(Value::as_array)
        {
            for option in options {
                let kind = option
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if kind == "allow_once" || kind == "allow_always" {
                    if let Some(option_id) = option.get("optionId").and_then(Value::as_str) {
                        return json!({ "outcome": "selected", "optionId": option_id });
                    }
                }
            }
        }
    }
    json!({ "outcome": "cancelled" })
}

/// Map a `session/update` notification's params to an [`AcpAgentEvent`], or
/// `None` for update kinds the runtime does not surface. Mirrors the Python
/// `_event_from_update`.
fn event_from_update(params: &Value) -> Option<AcpAgentEvent> {
    let update = params.get("update")?;
    let kind = update.get("sessionUpdate").and_then(Value::as_str)?;
    match kind {
        "agent_message_chunk" => {
            let text = content_text(update.get("content"));
            (!text.is_empty()).then(|| AcpAgentEvent::new(AcpEventKind::Token, Value::String(text)))
        }
        "agent_thought_chunk" => {
            let text = content_text(update.get("content"));
            (!text.is_empty())
                .then(|| AcpAgentEvent::new(AcpEventKind::Reasoning, Value::String(text)))
        }
        "tool_call" => Some(AcpAgentEvent::new(
            AcpEventKind::ToolCallStart,
            tool_start_payload(update),
        )),
        "tool_call_update" => Some(AcpAgentEvent::new(
            AcpEventKind::ToolCallResult,
            tool_progress_payload(update),
        )),
        "usage_update" => Some(AcpAgentEvent::new(
            AcpEventKind::Usage,
            json!({
                "used": update.get("used").cloned().unwrap_or(Value::Null),
                "size": update.get("size").cloned().unwrap_or(Value::Null),
            }),
        )),
        _ => None,
    }
}

/// Extract display text from a content block, mirroring Python `_content_text`.
fn content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match content.get("type").and_then(Value::as_str) {
        Some("text") => content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Some("resource") | Some("resource_link") => content
            .get("uri")
            .and_then(Value::as_str)
            .or_else(|| content.get("name").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string(),
        _ => content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

/// Build the `tool_call_start` payload, mirroring Python `_tool_start_payload`.
fn tool_start_payload(update: &Value) -> Value {
    let raw_status = update
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("started");
    let status = if raw_status == "pending" || raw_status == "in_progress" {
        "started"
    } else {
        raw_status
    };
    let args_source = update
        .get("rawInput")
        .filter(|v| !v.is_null())
        .or_else(|| update.get("kind").filter(|v| !v.is_null()));
    json!({
        "tool_call_id": update.get("toolCallId").cloned().unwrap_or(Value::Null),
        "tool_name": update.get("title").cloned().unwrap_or(Value::Null),
        "status": status,
        "args_summary": bounded_metadata(args_source),
    })
}

/// Build the `tool_call_result` payload, mirroring Python `_tool_progress_payload`.
fn tool_progress_payload(update: &Value) -> Value {
    let status = update
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let tool_name = update
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("ACP tool call");
    let result_source = update
        .get("rawOutput")
        .filter(|v| !v.is_null())
        .or_else(|| update.get("content").filter(|v| !v.is_null()));
    json!({
        "tool_call_id": update.get("toolCallId").cloned().unwrap_or(Value::Null),
        "tool_name": tool_name,
        "status": status,
        "result_summary": bounded_metadata(result_source),
    })
}

/// Stringify a tool input/output summary and cap it to [`MAX_METADATA_CHARS`],
/// mirroring Python `_bounded_metadata`.
fn bounded_metadata(value: Option<&Value>) -> String {
    let text = match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    if text.chars().count() <= MAX_METADATA_CHARS {
        return text;
    }
    let truncated: String = text.chars().take(MAX_METADATA_CHARS).collect();
    format!("{truncated}...")
}
