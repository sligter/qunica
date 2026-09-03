//! Server-owned terminal sessions.
//!
//! The desktop shell drives a PTY through Tauri commands, which the browser
//! build cannot reach. A remote deployment therefore needs the backend to own
//! the PTY and stream it over HTTP, so the same `TerminalPane` works whether
//! the app runs in a window or in a tab.
//!
//! Output is buffered per session: a client creates a session and *then* opens
//! the event stream, and the shell prompt is already written by the time that
//! second request lands. Every frame carries a sequence number, so a subscriber
//! can be handed the buffered history and the live tail without gaps or
//! duplicates.

use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::tools::{shell_for, ResolvedShell, ShellDialect, ShellPreference};

/// Sessions one account may hold open at once. A tab that is closed in the UI
/// releases its session, so this only bounds genuine simultaneous use.
const MAX_SESSIONS_PER_OWNER: usize = 12;
/// Replay budget per session. Enough for a screenful of scrollback on reconnect
/// without letting a chatty build log pin megabytes per tab.
const REPLAY_BUFFER_BYTES: usize = 256 * 1024;
/// How long a finished session stays addressable so a client that reconnects
/// still sees why its shell exited.
const FINISHED_SESSION_TTL: Duration = Duration::from_secs(300);
/// Live-tail capacity. A subscriber that falls further behind is caught up from
/// the replay buffer instead of losing output silently.
const EVENT_CHANNEL_CAPACITY: usize = 4096;
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// One frame of terminal activity, in the wire shape the frontend transport
/// already parses from the desktop channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "lowercase")]
pub enum TerminalEvent {
    Output {
        bytes_base64: String,
    },
    Exit {
        code: Option<i32>,
        signal: Option<String>,
    },
    Error {
        code: String,
        message: String,
    },
}

/// A frame paired with its per-session sequence number.
pub type TerminalFrame = (u64, TerminalEvent);

#[derive(Debug, Clone)]
pub struct TerminalError {
    pub code: &'static str,
    pub message: String,
}

impl TerminalError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalDescriptor {
    pub session_id: String,
    pub shell_name: String,
    pub cwd: String,
}

#[derive(Debug, Clone)]
pub struct CreateTerminalOptions {
    pub owner_id: String,
    pub conversation_id: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub shell: ShellPreference,
}

#[derive(Default)]
struct History {
    seq: u64,
    frames: VecDeque<TerminalFrame>,
    bytes: usize,
    finished_at: Option<Instant>,
}

pub struct TerminalSession {
    pub id: String,
    pub owner_id: String,
    pub conversation_id: String,
    pub shell_name: String,
    pub cwd: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    history: Mutex<History>,
    events: broadcast::Sender<TerminalFrame>,
}

/// Roughly what a frame costs to keep around. Only output has a payload worth
/// accounting for; the terminal frames are a fixed handful of bytes.
fn frame_cost(event: &TerminalEvent) -> usize {
    match event {
        TerminalEvent::Output { bytes_base64 } => bytes_base64.len(),
        _ => 64,
    }
}

impl TerminalSession {
    fn push(&self, event: TerminalEvent) {
        let mut history = self.history.lock().expect("terminal history mutex");
        history.seq += 1;
        let frame: TerminalFrame = (history.seq, event);
        if matches!(frame.1, TerminalEvent::Exit { .. }) {
            history.finished_at = Some(Instant::now());
        }
        history.bytes += frame_cost(&frame.1);
        history.frames.push_back(frame.clone());
        while history.bytes > REPLAY_BUFFER_BYTES && history.frames.len() > 1 {
            if let Some((_, dropped)) = history.frames.pop_front() {
                history.bytes = history.bytes.saturating_sub(frame_cost(&dropped));
            }
        }
        // Published while the lock is held so a subscriber can never observe a
        // sequence number on the channel that is not yet in the history.
        let _ = self.events.send(frame);
    }

    /// A live receiver plus everything buffered so far.
    ///
    /// The receiver is taken *before* the snapshot: a frame appended in between
    /// arrives on both, and the caller drops whatever it has already replayed by
    /// sequence number. Taking the snapshot first would lose that frame instead.
    pub fn subscribe(&self) -> (broadcast::Receiver<TerminalFrame>, Vec<TerminalFrame>) {
        let receiver = self.events.subscribe();
        let history = self.history.lock().expect("terminal history mutex");
        (receiver, history.frames.iter().cloned().collect())
    }

    /// Frames after `after_seq` that are still buffered. Used to recover a
    /// subscriber that fell behind the live channel.
    pub fn frames_after(&self, after_seq: u64) -> Vec<TerminalFrame> {
        let history = self.history.lock().expect("terminal history mutex");
        history
            .frames
            .iter()
            .filter(|(seq, _)| *seq > after_seq)
            .cloned()
            .collect()
    }

    pub fn is_finished(&self) -> bool {
        self.history
            .lock()
            .expect("terminal history mutex")
            .finished_at
            .is_some()
    }

    fn finished_for_longer_than(&self, ttl: Duration) -> bool {
        self.history
            .lock()
            .expect("terminal history mutex")
            .finished_at
            .is_some_and(|at| at.elapsed() > ttl)
    }

    pub fn write_input(&self, data: &[u8]) -> Result<(), TerminalError> {
        let mut writer = self.writer.lock().expect("terminal writer mutex");
        writer
            .write_all(data)
            .and_then(|()| writer.flush())
            .map_err(|err| TerminalError::new("terminal.write_failed", err.to_string()))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        self.master
            .lock()
            .expect("terminal master mutex")
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| TerminalError::new("terminal.resize_failed", err.to_string()))
    }

    fn kill(&self) {
        let _ = self.killer.lock().expect("terminal killer mutex").kill();
    }
}

#[derive(Default)]
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, Arc<TerminalSession>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn get(&self, session_id: &str, owner_id: &str) -> Option<Arc<TerminalSession>> {
        let sessions = self.sessions.lock().expect("terminal sessions mutex");
        sessions
            .get(session_id)
            .filter(|session| session.owner_id == owner_id)
            .cloned()
    }

    pub fn create(
        &self,
        options: CreateTerminalOptions,
    ) -> Result<Arc<TerminalSession>, TerminalError> {
        self.sweep_finished();

        let live = {
            let sessions = self.sessions.lock().expect("terminal sessions mutex");
            sessions
                .values()
                .filter(|session| session.owner_id == options.owner_id && !session.is_finished())
                .count()
        };
        if live >= MAX_SESSIONS_PER_OWNER {
            return Err(TerminalError::new(
                "terminal.too_many_sessions",
                format!("At most {MAX_SESSIONS_PER_OWNER} terminals can be open at once."),
            ));
        }

        let shell = resolve_shell(options.shell);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: options.rows.max(1),
                cols: options.cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| TerminalError::new("terminal.pty_open_failed", err.to_string()))?;

        let mut command = build_shell_command(&shell);
        command.cwd(&options.cwd);
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| TerminalError::new("terminal.shell_spawn_failed", err.to_string()))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| TerminalError::new("terminal.pty_reader_failed", err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| TerminalError::new("terminal.pty_writer_failed", err.to_string()))?;
        let killer = child.clone_killer();

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let session = Arc::new(TerminalSession {
            id: Uuid::new_v4().to_string(),
            owner_id: options.owner_id,
            conversation_id: options.conversation_id,
            shell_name: shell.display_name.clone(),
            cwd: options.cwd,
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            history: Mutex::new(History::default()),
            events,
        });

        // The PTY reader blocks, so it gets an OS thread rather than a runtime
        // worker; the same thread reaps the child once the pipe closes.
        let pump = Arc::clone(&session);
        std::thread::Builder::new()
            .name(format!("qunica-pty-{}", session.id))
            .spawn(move || {
                let mut reader = reader;
                let mut buffer = vec![0u8; READ_CHUNK_BYTES];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => pump.push(TerminalEvent::Output {
                            bytes_base64: BASE64.encode(&buffer[..read]),
                        }),
                    }
                }
                let code = child.wait().ok().map(|status| status.exit_code() as i32);
                // portable-pty reports a portable exit code only; a signalled
                // child surfaces as its wait status rather than a signal name.
                pump.push(TerminalEvent::Exit { code, signal: None });
            })
            .map_err(|err| TerminalError::new("terminal.reader_thread_failed", err.to_string()))?;

        self.sessions
            .lock()
            .expect("terminal sessions mutex")
            .insert(session.id.clone(), Arc::clone(&session));
        tracing::info!(
            session_id = %session.id,
            shell = %session.shell_name,
            cwd = %session.cwd,
            "terminal session started"
        );
        Ok(session)
    }

    pub fn close(&self, session_id: &str, owner_id: &str) -> bool {
        let removed = {
            let mut sessions = self.sessions.lock().expect("terminal sessions mutex");
            match sessions.get(session_id) {
                Some(session) if session.owner_id == owner_id => sessions.remove(session_id),
                _ => None,
            }
        };
        match removed {
            Some(session) => {
                session.kill();
                true
            }
            None => false,
        }
    }

    pub fn close_owned(&self, owner_id: &str) -> usize {
        let removed: Vec<Arc<TerminalSession>> = {
            let mut sessions = self.sessions.lock().expect("terminal sessions mutex");
            let ids: Vec<String> = sessions
                .values()
                .filter(|session| session.owner_id == owner_id)
                .map(|session| session.id.clone())
                .collect();
            ids.iter().filter_map(|id| sessions.remove(id)).collect()
        };
        for session in &removed {
            session.kill();
        }
        removed.len()
    }

    /// Drop sessions whose shell exited long enough ago that nobody is coming
    /// back for the output. Called on create so no background timer is needed.
    fn sweep_finished(&self) {
        let mut sessions = self.sessions.lock().expect("terminal sessions mutex");
        sessions.retain(|_, session| !session.finished_for_longer_than(FINISHED_SESSION_TTL));
    }
}

#[derive(Debug, Clone)]
struct ShellSpec {
    program: std::path::PathBuf,
    display_name: String,
}

/// The interpreter a new session starts, honouring the account preference.
///
/// A preference this host cannot satisfy falls back to the host default rather
/// than failing: a missing PowerShell should cost the tab its dialect, not its
/// ability to open at all.
fn resolve_shell(preference: ShellPreference) -> ShellSpec {
    let resolved = shell_for(preference);
    let requested = match preference {
        ShellPreference::Auto => None,
        ShellPreference::PowerShell => Some(ShellDialect::PowerShell),
        ShellPreference::Bash => Some(ShellDialect::Posix),
        ShellPreference::Cmd => Some(ShellDialect::Cmd),
    };
    if requested.is_some_and(|dialect| dialect != resolved.dialect) {
        let fallback = shell_for(ShellPreference::Auto);
        return ShellSpec {
            program: fallback.program.clone(),
            display_name: display_name(fallback),
        };
    }
    ShellSpec {
        program: resolved.program.clone(),
        display_name: display_name(resolved),
    }
}

fn display_name(shell: &ResolvedShell) -> String {
    match shell.dialect {
        ShellDialect::PowerShell => "PowerShell".to_string(),
        ShellDialect::Cmd => "Command Prompt".to_string(),
        ShellDialect::Posix => Path::new(&shell.program)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shell".to_string()),
    }
}

fn build_shell_command(shell: &ShellSpec) -> CommandBuilder {
    if cfg!(windows) {
        return CommandBuilder::new(shell.program.as_os_str());
    }
    // Mirrors the desktop shell: `new_default_prog` reads `SHELL` off the
    // builder, so setting it here is what selects the interpreter, and the
    // terminfo hints are what make xterm.js render colour.
    let mut command = CommandBuilder::new_default_prog();
    command.env("SHELL", shell.program.as_os_str());
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(cwd: &Path) -> CreateTerminalOptions {
        CreateTerminalOptions {
            owner_id: "owner".to_string(),
            conversation_id: "conversation".to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            cols: 80,
            rows: 24,
            shell: ShellPreference::Auto,
        }
    }

    #[test]
    fn terminal_session_replays_history_to_a_late_subscriber() {
        let dir = tempfile::tempdir().unwrap();
        let manager = TerminalManager::new();
        let session = manager.create(options(dir.path())).unwrap();

        session.push(TerminalEvent::Output {
            bytes_base64: BASE64.encode("hello"),
        });
        let (_receiver, replay) = session.subscribe();
        assert!(
            replay.iter().any(
                |(_, event)| matches!(event, TerminalEvent::Output { bytes_base64 }
                    if bytes_base64 == &BASE64.encode("hello"))
            ),
            "a subscriber that attaches after output must still receive it"
        );

        manager.close(&session.id, "owner");
    }

    #[test]
    fn terminal_manager_scopes_sessions_to_their_owner() {
        let dir = tempfile::tempdir().unwrap();
        let manager = TerminalManager::new();
        let session = manager.create(options(dir.path())).unwrap();

        assert!(manager.get(&session.id, "someone-else").is_none());
        assert!(!manager.close(&session.id, "someone-else"));
        assert!(manager.get(&session.id, "owner").is_some());
        assert!(manager.close(&session.id, "owner"));
        assert!(manager.get(&session.id, "owner").is_none());
    }

    #[test]
    fn terminal_history_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let manager = TerminalManager::new();
        let session = manager.create(options(dir.path())).unwrap();

        let chunk = BASE64.encode(vec![b'x'; READ_CHUNK_BYTES]);
        for _ in 0..64 {
            session.push(TerminalEvent::Output {
                bytes_base64: chunk.clone(),
            });
        }
        let buffered = session.history.lock().unwrap().bytes;
        assert!(
            buffered <= REPLAY_BUFFER_BYTES + chunk.len(),
            "history grew to {buffered} bytes"
        );

        manager.close(&session.id, "owner");
    }
}
