use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use super::manager::{EventSink, PtyHandle, PtySpawner};
use super::process_tree::ProcessTreeGuard;
use super::protocol::{CreateTerminalRequest, TerminalCommandError, TerminalEvent};
use super::shell::ShellSpec;

const OUTPUT_CHUNK_SIZE: usize = 16 * 1024;

#[derive(Default)]
pub struct NativePtySpawner;

struct PtyIo {
    master: Mutex<Option<Box<dyn MasterPty>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
}

impl PtyIo {
    fn write(&self, data: &[u8]) -> Result<(), TerminalCommandError> {
        let mut writer = self.writer.lock().map_err(|_| native_state_error())?;
        let writer = writer.as_mut().ok_or_else(|| {
            TerminalCommandError::new("terminal.session_closing", "Terminal session is closing")
        })?;
        writer.write_all(data).map_err(|_| {
            TerminalCommandError::new("terminal.write_failed", "Failed to write to terminal")
        })?;
        writer.flush().map_err(|_| {
            TerminalCommandError::new("terminal.write_failed", "Failed to flush terminal input")
        })
    }

    fn write_graceful_exit(&self) {
        let Ok(mut writer) = self.writer.lock() else {
            return;
        };
        if let Some(writer) = writer.as_mut() {
            let _ = writer.write_all(b"exit\r");
            let _ = writer.flush();
        }
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalCommandError> {
        let master = self.master.lock().map_err(|_| native_state_error())?;
        let master = master.as_ref().ok_or_else(|| {
            TerminalCommandError::new("terminal.session_closing", "Terminal session is closing")
        })?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|_| {
                TerminalCommandError::new("terminal.resize_failed", "Failed to resize terminal")
            })
    }

    fn close(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            writer.take();
        }
        if let Ok(mut master) = self.master.lock() {
            master.take();
        }
    }
}

struct CleanupState {
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    process_tree: Mutex<Option<ProcessTreeGuard>>,
    exited: Arc<AtomicBool>,
}

impl CleanupState {
    fn force(&self) -> Result<(), TerminalCommandError> {
        let root_alive = !self.exited.load(Ordering::Acquire);
        let guard = self
            .process_tree
            .lock()
            .map_err(|_| native_state_error())?
            .take();
        let process_tree_result = guard.map_or(Ok(()), |guard| guard.terminate(root_alive));

        if !self.exited.load(Ordering::Acquire) {
            if let Ok(mut killer) = self.killer.lock() {
                let _ = killer.kill();
            }
        }
        process_tree_result
    }
}

#[derive(Default)]
struct ReaderCompletion {
    done: Mutex<bool>,
    changed: Condvar,
}

impl ReaderCompletion {
    fn mark_done(&self) {
        if let Ok(mut done) = self.done.lock() {
            *done = true;
        }
        self.changed.notify_all();
    }

    fn wait(&self) {
        let Ok(mut done) = self.done.lock() else {
            return;
        };
        while !*done {
            let Ok(next) = self.changed.wait(done) else {
                return;
            };
            done = next;
        }
    }
}

struct NativePtyHandle {
    io: Arc<PtyIo>,
    cleanup: Arc<CleanupState>,
    exited: Arc<AtomicBool>,
    pid: u32,
}

impl NativePtySpawner {
    fn spawn_handle(
        &self,
        request: &CreateTerminalRequest,
        shell: &ShellSpec,
        sink: Arc<dyn EventSink>,
    ) -> Result<Arc<NativePtyHandle>, TerminalCommandError> {
        tracing::info!(
            lifecycle = "create",
            shell_kind = %shell.display_name,
            "creating terminal PTY"
        );

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|_| {
                TerminalCommandError::new("terminal.pty_open_failed", "Failed to open terminal PTY")
            })?;

        let mut command = CommandBuilder::new(shell.program.as_os_str());
        command.cwd(&request.cwd);
        let mut child = pair.slave.spawn_command(command).map_err(|_| {
            TerminalCommandError::new(
                "terminal.shell_spawn_failed",
                "Failed to start terminal shell",
            )
        })?;
        drop(pair.slave);

        let Some(pid) = child.process_id() else {
            let _ = child.kill();
            return Err(TerminalCommandError::new(
                "terminal.pid_unavailable",
                "Terminal shell did not expose a process ID",
            ));
        };

        let process_tree = match ProcessTreeGuard::attach(pid) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let _ = child.kill();
                return Err(error);
            }
        };
        let killer = child.clone_killer();

        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(_) => {
                let _ = child.kill();
                return Err(TerminalCommandError::new(
                    "terminal.pty_reader_failed",
                    "Failed to open terminal output stream",
                ));
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(_) => {
                let _ = child.kill();
                return Err(TerminalCommandError::new(
                    "terminal.pty_writer_failed",
                    "Failed to open terminal input stream",
                ));
            }
        };

        let exited = Arc::new(AtomicBool::new(false));
        let io = Arc::new(PtyIo {
            master: Mutex::new(Some(pair.master)),
            writer: Mutex::new(Some(writer)),
        });
        let cleanup = Arc::new(CleanupState {
            killer: Mutex::new(killer),
            process_tree: Mutex::new(Some(process_tree)),
            exited: exited.clone(),
        });
        let reader_completion = Arc::new(ReaderCompletion::default());

        let wait_sink = sink.clone();
        let wait_io = io.clone();
        let wait_cleanup = cleanup.clone();
        let wait_exited = exited.clone();
        let wait_reader_completion = reader_completion.clone();
        if thread::Builder::new()
            .name(format!("terminal-wait-{pid}"))
            .spawn(move || {
                let status = child.wait();
                wait_exited.store(true, Ordering::Release);
                wait_io.close();
                wait_reader_completion.wait();

                let event = match status {
                    Ok(status) => {
                        tracing::info!(
                            lifecycle = "exit",
                            exit_code = status.exit_code(),
                            signal = status.signal().unwrap_or("none"),
                            "terminal shell exited"
                        );
                        TerminalEvent::Exit {
                            code: Some(status.exit_code()),
                            signal: status.signal().map(str::to_string),
                        }
                    }
                    Err(_) => TerminalEvent::Error {
                        code: "terminal.wait_failed".to_string(),
                        message: "Failed to wait for terminal shell".to_string(),
                    },
                };
                if wait_sink.send(event).is_err() {
                    let result = wait_cleanup.force();
                    tracing::info!(
                        lifecycle = "close",
                        close_reason = "channel_disconnected",
                        forced_cleanup = result.is_ok(),
                        "terminal channel disconnected"
                    );
                }
            })
            .is_err()
        {
            let _ = cleanup.force();
            io.close();
            return Err(TerminalCommandError::new(
                "terminal.thread_spawn_failed",
                "Failed to start terminal wait thread",
            ));
        }

        let reader_sink = sink;
        let reader_io = io.clone();
        let reader_cleanup = cleanup.clone();
        let reader_completion_for_thread = reader_completion.clone();
        if thread::Builder::new()
            .name(format!("terminal-reader-{pid}"))
            .spawn(move || {
                read_output(
                    reader,
                    reader_sink,
                    reader_io,
                    reader_cleanup,
                    reader_completion_for_thread,
                );
            })
            .is_err()
        {
            reader_completion.mark_done();
            let _ = cleanup.force();
            io.close();
            return Err(TerminalCommandError::new(
                "terminal.thread_spawn_failed",
                "Failed to start terminal output thread",
            ));
        }

        tracing::info!(
            lifecycle = "created",
            shell_kind = %shell.display_name,
            process_id = pid,
            "terminal PTY created"
        );
        Ok(Arc::new(NativePtyHandle {
            io,
            cleanup,
            exited,
            pid,
        }))
    }
}

impl PtySpawner for NativePtySpawner {
    fn spawn(
        &self,
        request: &CreateTerminalRequest,
        shell: &ShellSpec,
        sink: Arc<dyn EventSink>,
    ) -> Result<Arc<dyn PtyHandle>, TerminalCommandError> {
        self.spawn_handle(request, shell, sink)
            .map(|handle| handle as Arc<dyn PtyHandle>)
    }
}

impl PtyHandle for NativePtyHandle {
    fn write(&self, data: &[u8]) -> Result<(), TerminalCommandError> {
        self.io.write(data)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalCommandError> {
        self.io.resize(cols, rows)
    }

    fn terminate(&self) -> Result<(), TerminalCommandError> {
        tracing::info!(
            lifecycle = "close",
            close_reason = "requested",
            process_id = self.pid,
            "closing terminal PTY"
        );
        self.io.write_graceful_exit();
        for _ in 0..20 {
            if self.exited.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }

        let result = self.cleanup.force();
        self.io.close();
        tracing::info!(
            lifecycle = "closed",
            forced_cleanup = result.is_ok(),
            process_id = self.pid,
            "terminal PTY cleanup finished"
        );
        result
    }
}

fn read_output(
    mut reader: Box<dyn Read + Send>,
    sink: Arc<dyn EventSink>,
    io: Arc<PtyIo>,
    cleanup: Arc<CleanupState>,
    completion: Arc<ReaderCompletion>,
) {
    let channel_connected = forward_output(reader.as_mut(), sink.as_ref());
    if !channel_connected {
        let result = cleanup.force();
        io.close();
        tracing::info!(
            lifecycle = "close",
            close_reason = "channel_disconnected",
            forced_cleanup = result.is_ok(),
            "terminal output channel disconnected"
        );
    }
    completion.mark_done();
}

fn forward_output(reader: &mut dyn Read, sink: &dyn EventSink) -> bool {
    let mut buffer = [0_u8; OUTPUT_CHUNK_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return true,
            Ok(read) => {
                if sink.send(output_event(&buffer[..read])).is_err() {
                    return false;
                }
            }
            Err(_) => return true,
        }
    }
}

fn output_event(bytes: &[u8]) -> TerminalEvent {
    TerminalEvent::Output {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

fn native_state_error() -> TerminalCommandError {
    TerminalCommandError::new(
        "terminal.native_state_failed",
        "Terminal native state is unavailable",
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use super::{forward_output, output_event, OUTPUT_CHUNK_SIZE};
    use crate::terminal::manager::EventSink;
    use crate::terminal::protocol::{TerminalCommandError, TerminalEvent};
    use base64::Engine;

    #[derive(Default)]
    struct RecordingSink {
        chunks: Mutex<Vec<Vec<u8>>>,
        fail_after: Option<usize>,
    }

    impl EventSink for RecordingSink {
        fn send(&self, event: TerminalEvent) -> Result<(), TerminalCommandError> {
            let mut chunks = self.chunks.lock().expect("recording chunks mutex poisoned");
            if self.fail_after.is_some_and(|limit| chunks.len() >= limit) {
                return Err(TerminalCommandError::new(
                    "terminal.channel_disconnected",
                    "test channel disconnected",
                ));
            }
            let TerminalEvent::Output { bytes_base64 } = event else {
                panic!("expected output event");
            };
            chunks.push(
                base64::engine::general_purpose::STANDARD
                    .decode(bytes_base64)
                    .expect("output should be valid base64"),
            );
            Ok(())
        }
    }

    #[test]
    fn output_chunks_round_trip_as_bytes() {
        let bytes = [0xf0, 0x9f, 0x98, 0x80, b'\r', b'\n'];
        let event = output_event(&bytes);
        let TerminalEvent::Output { bytes_base64 } = event else {
            panic!("expected output");
        };
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(bytes_base64)
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn output_stream_is_bounded_to_sixteen_kibibyte_events() {
        let bytes = (0..(OUTPUT_CHUNK_SIZE * 2 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let mut reader = Cursor::new(bytes.clone());
        let sink = RecordingSink::default();

        assert!(forward_output(&mut reader, &sink));

        let chunks = sink.chunks.lock().expect("recording chunks mutex poisoned");
        assert_eq!(chunks.iter().map(Vec::len).max(), Some(OUTPUT_CHUNK_SIZE));
        assert_eq!(chunks.concat(), bytes);
    }

    #[test]
    fn output_stream_stops_when_the_channel_disconnects() {
        let mut reader = Cursor::new(vec![b'x'; OUTPUT_CHUNK_SIZE * 3]);
        let sink = RecordingSink {
            chunks: Mutex::new(Vec::new()),
            fail_after: Some(1),
        };

        assert!(!forward_output(&mut reader, &sink));
        assert_eq!(
            sink.chunks
                .lock()
                .expect("recording chunks mutex poisoned")
                .len(),
            1
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "manual Windows PTY smoke test"]
    fn windows_real_pty_smoke() {
        windows_smoke::run_windows_real_pty_smoke();
    }

    #[cfg(windows)]
    mod windows_smoke {
        use std::sync::{Arc, Condvar, Mutex};
        use std::time::{Duration, Instant};

        use base64::Engine;
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        use super::super::{NativePtySpawner, PtyHandle};
        use crate::terminal::manager::EventSink;
        use crate::terminal::protocol::{
            CreateTerminalRequest, TerminalCommandError, TerminalEvent,
        };
        use crate::terminal::shell::resolve_default_shell;

        #[derive(Default)]
        struct RecordingState {
            bytes: Vec<u8>,
            exited: bool,
        }

        #[derive(Default)]
        struct RecordingSink {
            state: Mutex<RecordingState>,
            changed: Condvar,
        }

        impl RecordingSink {
            fn wait_for_marker(&self, marker: &str) -> String {
                let deadline = Instant::now() + Duration::from_secs(15);
                let mut state = self.state.lock().expect("recording sink mutex poisoned");
                loop {
                    let output = String::from_utf8_lossy(&state.bytes).into_owned();
                    if output.contains(marker) {
                        return output;
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    assert!(
                        !remaining.is_zero(),
                        "timed out waiting for {marker}: {output}"
                    );
                    let (next, timeout) = self
                        .changed
                        .wait_timeout(state, remaining)
                        .expect("recording sink mutex poisoned");
                    state = next;
                    assert!(
                        !timeout.timed_out(),
                        "timed out waiting for {marker}: {output}"
                    );
                }
            }

            fn wait_for_exit(&self) {
                let deadline = Instant::now() + Duration::from_secs(15);
                let mut state = self.state.lock().expect("recording sink mutex poisoned");
                while !state.exited {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    assert!(!remaining.is_zero(), "timed out waiting for terminal exit");
                    let (next, timeout) = self
                        .changed
                        .wait_timeout(state, remaining)
                        .expect("recording sink mutex poisoned");
                    state = next;
                    assert!(!timeout.timed_out(), "timed out waiting for terminal exit");
                }
            }
        }

        impl EventSink for RecordingSink {
            fn send(&self, event: TerminalEvent) -> Result<(), TerminalCommandError> {
                let mut state = self.state.lock().expect("recording sink mutex poisoned");
                match event {
                    TerminalEvent::Output { bytes_base64 } => {
                        state.bytes.extend(
                            base64::engine::general_purpose::STANDARD
                                .decode(bytes_base64)
                                .expect("PTY output should be valid base64"),
                        );
                    }
                    TerminalEvent::Exit { .. } => state.exited = true,
                    TerminalEvent::Error { code, message } => {
                        panic!("terminal smoke test error {code}: {message}")
                    }
                }
                self.changed.notify_all();
                Ok(())
            }
        }

        pub(super) fn run_windows_real_pty_smoke() {
            let temp = std::env::temp_dir()
                .join(format!("ag-swarmer-terminal-smoke-{}", std::process::id()));
            std::fs::create_dir_all(&temp).expect("create smoke-test directory");
            let shell = resolve_default_shell().expect("PowerShell should resolve");
            assert_eq!(shell.display_name, "PowerShell");
            let request = CreateTerminalRequest {
                conversation_id: "smoke-test".to_string(),
                cwd: temp.to_string_lossy().into_owned(),
                cols: 80,
                rows: 24,
            };
            let sink = Arc::new(RecordingSink::default());
            let handle = NativePtySpawner
                .spawn_handle(&request, &shell, sink.clone())
                .expect("spawn real PTY");
            let root_pid = handle.pid;

            handle
                .write(b"\x1b[1;1R")
                .expect("answer ConPTY cursor-position query");
            handle
                .write(b"Write-Output PTY_OK\r")
                .expect("write smoke marker");
            sink.wait_for_marker("PTY_OK");
            handle.resize(100, 30).expect("resize real PTY");

            handle
                .write(b"$child = Start-Process powershell.exe -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 300' -PassThru; Write-Output (('DESCENDANT_' + 'PID=') + $child.Id)\r")
                .expect("start descendant process");
            let output = sink.wait_for_marker("DESCENDANT_PID=");
            let descendant_pid = parse_pid_marker(&output, "DESCENDANT_PID=");
            assert!(process_is_alive(descendant_pid));

            handle.write(b"exit\r").expect("exit root shell");
            sink.wait_for_exit();
            wait_until_stopped(root_pid);
            assert!(process_is_alive(descendant_pid));

            handle.terminate().expect("close terminal Job Object");
            wait_until_stopped(descendant_pid);
            wait_until_stopped(root_pid);
            remove_directory_when_released(&temp);
        }

        fn parse_pid_marker(output: &str, marker: &str) -> u32 {
            output
                .match_indices(marker)
                .filter_map(|(index, _)| {
                    let digits = output[index + marker.len()..]
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>();
                    (!digits.is_empty())
                        .then(|| digits.parse::<u32>().ok())
                        .flatten()
                })
                .next()
                .expect("output should contain a numeric descendant PID")
        }

        fn wait_until_stopped(pid: u32) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while process_is_alive(pid) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(!process_is_alive(pid), "process {pid} should be stopped");
        }

        fn remove_directory_when_released(path: &std::path::Path) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match std::fs::remove_dir_all(path) {
                    Ok(()) => return,
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(error) => panic!("remove smoke-test directory: {error}"),
                }
            }
        }

        fn process_is_alive(pid: u32) -> bool {
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if process.is_null() {
                return false;
            }
            let mut code = 0;
            let queried = unsafe { GetExitCodeProcess(process, &mut code) };
            let _ = unsafe { CloseHandle(process) };
            queried != 0 && code == STILL_ACTIVE as u32
        }
    }
}
