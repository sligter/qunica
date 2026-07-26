//! stdio transport: a local child process speaking newline-delimited JSON-RPC.
//!
//! # Wire framing
//!
//! Each message is one compact JSON object terminated by `\n`, UTF-8 encoded,
//! written to the child's stdin and read from its stdout. There is no
//! `Content-Length` header — MCP's stdio transport uses line framing, the same
//! as the ACP transport in [`crate::acp::protocol`].
//!
//! The child's stdout is reserved for protocol traffic, but real servers still
//! print banners there. The reader skips any line that is not a JSON object
//! rather than treating it as a protocol violation. stderr is drained into a
//! bounded tail so a startup failure can be reported with the server's own error
//! text instead of a bare "unreachable".

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Child,
    sync::{mpsc, oneshot, Mutex},
    task::JoinHandle,
    time::timeout,
};

use crate::acp::process::Tail;
use crate::mcp::{
    config::McpServerConfig,
    protocol::{method_not_found_response, notification_envelope, request_envelope},
    McpError,
};
use crate::process::tokio_command_no_window;

/// How long to wait for the reader/writer tasks to wind down on close.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>;

/// Messages handed to the background writer task.
enum WriterMessage {
    Line(String),
    Close,
}

/// A running MCP server child process plus its protocol tasks.
pub struct StdioTransport {
    writer_tx: mpsc::UnboundedSender<WriterMessage>,
    pending: PendingMap,
    next_id: Arc<AtomicI64>,
    timeout_seconds: u64,
    alive: Arc<AtomicBool>,
    stderr_tail: Arc<Mutex<Tail>>,
    child: Arc<Mutex<Child>>,
    reader: Mutex<Option<JoinHandle<()>>>,
    writer: Mutex<Option<JoinHandle<()>>>,
    stderr_reader: Mutex<Option<JoinHandle<()>>>,
}

impl StdioTransport {
    /// Spawn the configured command and start the protocol tasks.
    ///
    /// The child inherits this process's environment (so `PATH` resolves) with
    /// the configured overlay applied on top. Unlike the ACP runtime, no home
    /// directory isolation is imposed: an MCP server is explicitly configured by
    /// the operator and commonly needs host credentials to do its job.
    pub fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let command = config
            .command
            .as_deref()
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| McpError::Config("a stdio server needs a command to run".to_string()))?;

        let cwd = resolve_cwd(config.cwd.as_deref())?;

        let (launch_command, launch_args) = launch_command(command, &config.args);
        let mut std_cmd = StdCommand::new(launch_command);
        std_cmd.args(launch_args);
        if let Some(cwd) = cwd.as_deref() {
            std_cmd.current_dir(cwd);
        }
        for (key, value) in &config.env {
            std_cmd.env(key, value);
        }
        std_cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut cmd = tokio_command_no_window(std_cmd);
        cmd.kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|err| McpError::Transport(format!("could not start '{command}': {err}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("child stdin pipe missing".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("child stdout pipe missing".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| McpError::Transport("child stderr pipe missing".to_string()))?;

        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<WriterMessage>();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let stderr_tail = Arc::new(Mutex::new(Tail::new()));

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
            // Dropping stdin closes the child's input, signalling EOF.
        });

        let reader_pending = pending.clone();
        let reader_writer_tx = writer_tx.clone();
        let reader_alive = alive.clone();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                route_incoming(&line, &reader_pending, &reader_writer_tx).await;
            }
            // stdout EOF means the child is gone. Drop every pending sender so
            // in-flight requests resolve to `TransportClosed` instead of waiting
            // out their full timeout.
            reader_alive.store(false, Ordering::SeqCst);
            reader_pending.lock().await.clear();
        });

        let stderr_sink = stderr_tail.clone();
        let stderr_reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut tail = stderr_sink.lock().await;
                tail.append(&line);
                tail.append("\n");
            }
        });

        Ok(Self {
            writer_tx,
            pending,
            next_id: Arc::new(AtomicI64::new(1)),
            timeout_seconds: config.effective_timeout(),
            alive,
            stderr_tail,
            child: Arc::new(Mutex::new(child)),
            reader: Mutex::new(Some(reader)),
            writer: Mutex::new(Some(writer)),
            stderr_reader: Mutex::new(Some(stderr_reader)),
        })
    }

    /// The bounded stderr tail captured so far, for diagnostics.
    pub async fn stderr_tail(&self) -> Option<String> {
        let text = self.stderr_tail.lock().await.snapshot().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Build a transport error that carries the child's own stderr when it has
    /// written any, so "unreachable" becomes actionable.
    async fn dead_error(&self) -> McpError {
        match self.stderr_tail().await {
            Some(tail) => McpError::Transport(format!(
                "the server process exited. Its last output was: {}",
                tail.trim()
            )),
            None => McpError::Transport("the server process exited".to_string()),
        }
    }
}

#[async_trait]
impl super::McpTransport for StdioTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(self.dead_error().await);
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let line = encode_line(&request_envelope(id, method, params));
        if self.writer_tx.send(WriterMessage::Line(line)).is_err() {
            self.pending.lock().await.remove(&id);
            self.alive.store(false, Ordering::SeqCst);
            return Err(self.dead_error().await);
        }

        match timeout(Duration::from_secs(self.timeout_seconds), rx).await {
            Ok(Ok(message)) => Ok(message),
            // The sender was dropped: the reader saw stdout EOF.
            Ok(Err(_)) => Err(self.dead_error().await),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(McpError::Timeout(self.timeout_seconds))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let line = encode_line(&notification_envelope(method, params));
        self.writer_tx
            .send(WriterMessage::Line(line))
            .map_err(|_| McpError::Transport("the server process exited".to_string()))
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let _ = self.writer_tx.send(WriterMessage::Close);

        // Closing stdin is the protocol's shutdown signal; give the child a
        // moment to exit on its own before killing it.
        let exited = {
            let mut child = self.child.lock().await;
            timeout(SHUTDOWN_GRACE, child.wait()).await.is_ok()
        };
        if !exited {
            let mut child = self.child.lock().await;
            let _ = child.start_kill();
            let _ = timeout(SHUTDOWN_GRACE, child.wait()).await;
        }

        for handle in [&self.reader, &self.writer, &self.stderr_reader] {
            let Some(mut task) = handle.lock().await.take() else {
                continue;
            };
            // The child is already dead or being killed, so its pipes close and
            // the tasks end on their own. Abort only if one somehow lingers, so
            // shutdown can never hang.
            if timeout(SHUTDOWN_GRACE, &mut task).await.is_err() {
                task.abort();
            }
        }
        self.pending.lock().await.clear();
    }
}

/// Serialize a JSON value to one compact line terminated by `\n`.
fn encode_line(value: &Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

/// Route one stdout line. Non-JSON-object lines are server chatter and skipped.
async fn route_incoming(
    line: &str,
    pending: &PendingMap,
    writer_tx: &mpsc::UnboundedSender<WriterMessage>,
) {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return;
    }
    let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
        return;
    };

    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id").filter(|id| !id.is_null());

    match (method, id) {
        // A server-initiated request. Nothing beyond tools is supported, so
        // answer every one with method-not-found rather than leaving it hanging.
        (Some(method), Some(id)) => {
            let response = method_not_found_response(id, method);
            let _ = writer_tx.send(WriterMessage::Line(encode_line(&response)));
        }
        // A notification. `notifications/tools/list_changed` would matter if
        // tool lists were cached across turns; they are re-listed per turn, so
        // nothing needs doing here.
        (Some(_), None) => {}
        // A response to one of our requests.
        (None, Some(id)) => {
            if let Some(id) = id.as_i64() {
                if let Some(tx) = pending.lock().await.remove(&id) {
                    let _ = tx.send(message);
                }
            }
        }
        (None, None) => {}
    }
}

/// Validate a configured working directory, or return `None` to inherit.
fn resolve_cwd(cwd: Option<&str>) -> Result<Option<PathBuf>, McpError> {
    let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(cwd);
    if !path.is_dir() {
        return Err(McpError::Config(
            "the configured working directory does not exist".to_string(),
        ));
    }
    Ok(Some(path))
}

/// Resolve the executable to launch.
///
/// On Windows, npm installs global CLIs as `.cmd` shims that cannot be executed
/// directly by `CreateProcess`; they are run through `cmd /c call` instead. This
/// mirrors the ACP runtime's handling, which the same class of servers (`npx`,
/// package-installed binaries) needs.
fn launch_command(command: &str, args: &[String]) -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    {
        let is_batch = matches!(
            Path::new(command)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some(extension)
                if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        );
        // A bare `npx`/`pnpm` with no extension also resolves to a `.cmd` shim
        // through `PATHEXT`, which `CreateProcess` will not honour either.
        let is_extensionless_shim = Path::new(command).extension().is_none()
            && matches!(
                command.to_ascii_lowercase().as_str(),
                "npx" | "npm" | "pnpm" | "yarn" | "bunx"
            );
        if is_batch || is_extensionless_shim {
            let comspec = std::env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
            let mut launch_args = vec![
                "/d".to_string(),
                "/c".to_string(),
                "call".to_string(),
                command.to_string(),
            ];
            launch_args.extend(args.iter().cloned());
            return (comspec, launch_args);
        }
    }
    #[cfg(not(windows))]
    let _ = Path::new(command);

    (PathBuf::from(command), args.to_vec())
}

#[cfg(test)]
mod tests {
    use super::{encode_line, launch_command, resolve_cwd};
    use serde_json::json;

    #[test]
    fn lines_are_compact_and_newline_terminated() {
        let line = encode_line(&json!({"jsonrpc":"2.0","id":1}));
        assert!(line.ends_with('\n'));
        assert!(!line.trim().contains(' '));
    }

    #[test]
    fn a_blank_working_directory_inherits_the_parents() {
        assert_eq!(resolve_cwd(None).unwrap(), None);
        assert_eq!(resolve_cwd(Some("   ")).unwrap(), None);
    }

    #[test]
    fn a_missing_working_directory_is_rejected() {
        assert!(resolve_cwd(Some("/definitely/not/a/real/directory/xyzzy")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_runs_npx_through_the_command_processor() {
        let (command, args) = launch_command("npx", &["-y".to_string(), "server".to_string()]);
        assert!(command.to_string_lossy().to_lowercase().ends_with("cmd.exe"));
        assert_eq!(args, vec!["/d", "/c", "call", "npx", "-y", "server"]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_leaves_real_executables_alone() {
        let (command, args) = launch_command("python.exe", &["-m".to_string()]);
        assert_eq!(command.to_string_lossy(), "python.exe");
        assert_eq!(args, vec!["-m"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn other_platforms_launch_the_command_directly() {
        let (command, args) = launch_command("npx", &["-y".to_string()]);
        assert_eq!(command.to_string_lossy(), "npx");
        assert_eq!(args, vec!["-y"]);
    }
}
