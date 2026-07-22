use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine;
use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};

use super::manager::{EventSink, PtyHandle, PtySpawner};
use super::process_tree::{attach_or_rollback, ProcessTreeGuard};
use super::protocol::{CreateTerminalRequest, TerminalCommandError, TerminalEvent};
use super::shell::ShellSpec;

const OUTPUT_CHUNK_SIZE: usize = 16 * 1024;
const INPUT_CHUNK_SIZE: usize = 16 * 1024;
const INPUT_QUEUE_CAPACITY: usize = 64;
const GRACEFUL_EXIT_TIMEOUT: Duration = Duration::from_millis(500);
const TAIL_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct NativePtySpawner;

struct PtyIo {
    master: Mutex<Option<Box<dyn MasterPty>>>,
}

impl PtyIo {
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
        if let Ok(mut master) = self.master.lock() {
            master.take();
        }
    }
}

struct InputQueue {
    sender: Mutex<Option<SyncSender<Vec<u8>>>>,
    closed: Arc<AtomicBool>,
}

impl InputQueue {
    fn new(sender: SyncSender<Vec<u8>>) -> Self {
        Self {
            sender: Mutex::new(Some(sender)),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn closed_flag(&self) -> Arc<AtomicBool> {
        self.closed.clone()
    }

    fn enqueue(&self, data: &[u8]) -> Result<(), TerminalCommandError> {
        if data.len() > INPUT_CHUNK_SIZE {
            return Err(TerminalCommandError::new(
                "terminal.input_too_large",
                "Terminal input exceeds the 16 KiB limit",
            ));
        }
        if self.closed.load(Ordering::Acquire) {
            return Err(session_closing_error());
        }
        let sender = self.sender.lock().map_err(|_| native_state_error())?;
        let sender = sender.as_ref().ok_or_else(session_closing_error)?;
        sender.try_send(data.to_vec()).map_err(|error| match error {
            TrySendError::Full(_) => TerminalCommandError::new(
                "terminal.input_backpressure",
                "Terminal input queue is full",
            ),
            TrySendError::Disconnected(_) => session_closing_error(),
        })
    }

    fn try_graceful_exit(&self) {
        let Ok(sender) = self.sender.lock() else {
            return;
        };
        if let Some(sender) = sender.as_ref() {
            let _ = sender.try_send(b"exit\r".to_vec());
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
    }
}

#[derive(Default)]
struct RootCompletion {
    exited: Mutex<bool>,
    changed: Condvar,
}

impl RootCompletion {
    fn mark_exited(&self) {
        if let Ok(mut exited) = self.exited.lock() {
            *exited = true;
        }
        self.changed.notify_all();
    }

    fn has_exited(&self) -> bool {
        self.exited.lock().is_ok_and(|exited| *exited)
    }

    fn wait_for(&self, timeout: Duration) {
        let Ok(exited) = self.exited.lock() else {
            return;
        };
        if !*exited {
            let _ = self.changed.wait_timeout(exited, timeout);
        }
    }
}

struct CleanupState {
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    process_tree: Mutex<Option<ProcessTreeGuard>>,
    root: Arc<RootCompletion>,
}

impl CleanupState {
    fn force(&self) -> Result<(), TerminalCommandError> {
        let root_alive = !self.root.has_exited();
        let mut guard_slot = self.process_tree.lock().map_err(|_| native_state_error())?;
        let process_tree_result = guard_slot
            .as_mut()
            .map_or(Ok(()), |guard| guard.terminate(root_alive));
        if process_tree_result.is_ok() {
            guard_slot.take();
        }
        drop(guard_slot);

        if !self.root.has_exited() {
            if let Ok(mut killer) = self.killer.lock() {
                let _ = killer.kill();
            }
        }
        process_tree_result
    }

    fn on_root_exit(&self) -> Result<(), TerminalCommandError> {
        let mut guard_slot = self.process_tree.lock().map_err(|_| native_state_error())?;
        let release = match guard_slot.as_mut() {
            Some(guard) => guard.on_root_exit()?,
            None => false,
        };
        if release {
            guard_slot.take();
        }
        Ok(())
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

    fn wait_for(&self, timeout: Duration) -> bool {
        let Ok(mut done) = self.done.lock() else {
            return true;
        };
        if !*done {
            let Ok((next, _)) = self.changed.wait_timeout(done, timeout) else {
                return true;
            };
            done = next;
        }
        *done
    }
}

#[derive(Default)]
struct StartupGate {
    state: Mutex<StartupState>,
    changed: Condvar,
}

#[derive(Default)]
enum StartupState {
    #[default]
    Pending,
    Committed,
    Aborted,
}

impl StartupGate {
    fn commit(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = StartupState::Committed;
        }
        self.changed.notify_all();
    }

    fn abort(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = StartupState::Aborted;
        }
        self.changed.notify_all();
    }

    fn wait_for_commit(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        while matches!(*state, StartupState::Pending) {
            let Ok(next) = self.changed.wait(state) else {
                return false;
            };
            state = next;
        }
        matches!(*state, StartupState::Committed)
    }
}

struct EventDispatcher {
    sink: Arc<dyn EventSink>,
    state: Mutex<EventState>,
}

#[derive(Default)]
struct EventState {
    exit_sent: bool,
}

impl EventDispatcher {
    fn new(sink: Arc<dyn EventSink>) -> Self {
        Self {
            sink,
            state: Mutex::new(EventState::default()),
        }
    }

    fn send_output(&self, bytes: &[u8]) -> Result<(), TerminalCommandError> {
        let state = self.state.lock().map_err(|_| native_state_error())?;
        if state.exit_sent {
            return Ok(());
        }
        self.sink.send(output_event(bytes))
    }

    fn send_error(
        &self,
        code: &'static str,
        message: &'static str,
    ) -> Result<(), TerminalCommandError> {
        let state = self.state.lock().map_err(|_| native_state_error())?;
        if state.exit_sent {
            return Ok(());
        }
        self.sink.send(TerminalEvent::Error {
            code: code.to_string(),
            message: message.to_string(),
        })
    }

    fn send_exit(
        &self,
        code: Option<u32>,
        signal: Option<String>,
    ) -> Result<(), TerminalCommandError> {
        let mut state = self.state.lock().map_err(|_| native_state_error())?;
        if state.exit_sent {
            return Ok(());
        }
        state.exit_sent = true;
        self.sink.send(TerminalEvent::Exit { code, signal })
    }
}

struct NativePtyHandle {
    io: Arc<PtyIo>,
    input: Arc<InputQueue>,
    cleanup: Arc<CleanupState>,
    root: Arc<RootCompletion>,
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

        let process_tree = attach_or_rollback(pid, child.as_mut())?;
        let killer = child.clone_killer();
        let root = Arc::new(RootCompletion::default());
        let cleanup = Arc::new(CleanupState {
            killer: Mutex::new(killer),
            process_tree: Mutex::new(Some(process_tree)),
            root: root.clone(),
        });

        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(_) => {
                rollback_attached_child(cleanup.as_ref(), child.as_mut());
                return Err(TerminalCommandError::new(
                    "terminal.pty_reader_failed",
                    "Failed to open terminal output stream",
                ));
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(_) => {
                rollback_attached_child(cleanup.as_ref(), child.as_mut());
                return Err(TerminalCommandError::new(
                    "terminal.pty_writer_failed",
                    "Failed to open terminal input stream",
                ));
            }
        };

        let io = Arc::new(PtyIo {
            master: Mutex::new(Some(pair.master)),
        });
        let (input_sender, input_receiver) = sync_channel(INPUT_QUEUE_CAPACITY);
        let input = Arc::new(InputQueue::new(input_sender));
        let input_closed = input.closed_flag();
        let dispatcher = Arc::new(EventDispatcher::new(sink));
        let reader_completion = Arc::new(ReaderCompletion::default());
        let startup = Arc::new(StartupGate::default());
        let child_slot = Arc::new(Mutex::new(Some(child)));

        let writer_startup = startup.clone();
        let writer_dispatcher = dispatcher.clone();
        let wait_io = io.clone();
        let writer_cleanup = cleanup.clone();
        if thread::Builder::new()
            .name(format!("terminal-writer-{pid}"))
            .spawn(move || {
                if writer_startup.wait_for_commit() {
                    write_input(
                        input_receiver,
                        input_closed,
                        writer,
                        writer_dispatcher,
                        wait_io,
                        writer_cleanup,
                    );
                }
            })
            .is_err()
        {
            abort_startup(&startup, &input, &io, &cleanup, &child_slot);
            return Err(TerminalCommandError::new(
                "terminal.thread_spawn_failed",
                "Failed to start terminal input thread",
            ));
        }

        let reader_startup = startup.clone();
        let reader_dispatcher = dispatcher.clone();
        let reader_io = io.clone();
        let reader_cleanup = cleanup.clone();
        let reader_completion_for_thread = reader_completion.clone();
        if thread::Builder::new()
            .name(format!("terminal-reader-{pid}"))
            .spawn(move || {
                if reader_startup.wait_for_commit() {
                    read_output(
                        reader,
                        reader_dispatcher,
                        reader_io,
                        reader_cleanup,
                        reader_completion_for_thread,
                    );
                } else {
                    reader_completion_for_thread.mark_done();
                }
            })
            .is_err()
        {
            reader_completion.mark_done();
            abort_startup(&startup, &input, &io, &cleanup, &child_slot);
            return Err(TerminalCommandError::new(
                "terminal.thread_spawn_failed",
                "Failed to start terminal output thread",
            ));
        }

        let wait_startup = startup.clone();
        let wait_dispatcher = dispatcher;
        let wait_io = io.clone();
        let wait_input = input.clone();
        let wait_cleanup = cleanup.clone();
        let wait_root = root.clone();
        let wait_reader_completion = reader_completion;
        let wait_child_slot = child_slot.clone();
        if thread::Builder::new()
            .name(format!("terminal-wait-{pid}"))
            .spawn(move || {
                if !wait_startup.wait_for_commit() {
                    return;
                }
                wait_for_child(
                    wait_child_slot,
                    wait_dispatcher,
                    wait_io,
                    wait_input,
                    wait_cleanup,
                    wait_root,
                    wait_reader_completion,
                );
            })
            .is_err()
        {
            abort_startup(&startup, &input, &io, &cleanup, &child_slot);
            return Err(TerminalCommandError::new(
                "terminal.thread_spawn_failed",
                "Failed to start terminal wait thread",
            ));
        }

        startup.commit();

        tracing::info!(
            lifecycle = "created",
            shell_kind = %shell.display_name,
            process_id = pid,
            "terminal PTY created"
        );
        Ok(Arc::new(NativePtyHandle {
            io,
            input,
            cleanup,
            root,
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
        self.input.enqueue(data)
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
        self.input.try_graceful_exit();
        self.root.wait_for(GRACEFUL_EXIT_TIMEOUT);

        self.input.close();
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

fn rollback_attached_child(cleanup: &CleanupState, child: &mut dyn Child) {
    let _ = cleanup.force();
    let _ = child.kill();
    let _ = child.wait();
}

fn abort_startup(
    startup: &StartupGate,
    input: &InputQueue,
    io: &PtyIo,
    cleanup: &CleanupState,
    child_slot: &Mutex<Option<Box<dyn Child + Send + Sync>>>,
) {
    startup.abort();
    input.close();
    let _ = cleanup.force();
    io.close();
    if let Ok(mut slot) = child_slot.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn write_input(
    receiver: Receiver<Vec<u8>>,
    input_closed: Arc<AtomicBool>,
    mut writer: Box<dyn Write + Send>,
    dispatcher: Arc<EventDispatcher>,
    io: Arc<PtyIo>,
    cleanup: Arc<CleanupState>,
) {
    while let Ok(data) = receiver.recv() {
        if input_closed.load(Ordering::Acquire) {
            break;
        }
        if writer
            .write_all(&data)
            .and_then(|()| writer.flush())
            .is_ok()
        {
            continue;
        }

        let _ = dispatcher.send_error("terminal.write_failed", "Failed to write to terminal");
        let result = cleanup.force();
        io.close();
        tracing::info!(
            lifecycle = "close",
            close_reason = "write_failed",
            forced_cleanup = result.is_ok(),
            "terminal input writer failed"
        );
        break;
    }
}

fn wait_for_child(
    child_slot: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
    dispatcher: Arc<EventDispatcher>,
    io: Arc<PtyIo>,
    input: Arc<InputQueue>,
    cleanup: Arc<CleanupState>,
    root: Arc<RootCompletion>,
    reader_completion: Arc<ReaderCompletion>,
) {
    let Some(mut child) = child_slot.lock().ok().and_then(|mut slot| slot.take()) else {
        return;
    };
    let status = child.wait();
    if status.is_ok() {
        root.mark_exited();
    }
    input.close();

    let cleanup_result = if status.is_ok() {
        cleanup.on_root_exit()
    } else {
        cleanup.force()
    };
    io.close();
    let _ = reader_completion.wait_for(TAIL_OUTPUT_DRAIN_TIMEOUT);

    if cleanup_result.is_err()
        && dispatcher
            .send_error(
                "terminal.process_tree_failed",
                "Failed to clean up the terminal process tree",
            )
            .is_err()
    {
        let _ = cleanup.force();
        return;
    }

    let (code, signal) = match status {
        Ok(status) => {
            tracing::info!(
                lifecycle = "exit",
                exit_code = status.exit_code(),
                signal = status.signal().unwrap_or("none"),
                "terminal shell exited"
            );
            (
                Some(status.exit_code()),
                status.signal().map(str::to_string),
            )
        }
        Err(_) => {
            if dispatcher
                .send_error("terminal.wait_failed", "Failed to wait for terminal shell")
                .is_err()
            {
                let _ = cleanup.force();
                return;
            }
            (None, None)
        }
    };
    if dispatcher.send_exit(code, signal).is_err() {
        let result = cleanup.force();
        tracing::info!(
            lifecycle = "close",
            close_reason = "channel_disconnected",
            forced_cleanup = result.is_ok(),
            "terminal channel disconnected"
        );
    }
}

fn read_output(
    mut reader: Box<dyn Read + Send>,
    dispatcher: Arc<EventDispatcher>,
    io: Arc<PtyIo>,
    cleanup: Arc<CleanupState>,
    completion: Arc<ReaderCompletion>,
) {
    let channel_connected = forward_dispatched_output(reader.as_mut(), dispatcher.as_ref());
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

fn forward_dispatched_output(reader: &mut dyn Read, dispatcher: &EventDispatcher) -> bool {
    let mut buffer = [0_u8; OUTPUT_CHUNK_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return true,
            Ok(read) => {
                if dispatcher.send_output(&buffer[..read]).is_err() {
                    return false;
                }
            }
            Err(_) => return true,
        }
    }
}

#[cfg(test)]
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

fn session_closing_error() -> TerminalCommandError {
    TerminalCommandError::new("terminal.session_closing", "Terminal session is closing")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::mpsc::sync_channel;
    use std::sync::{Arc, Condvar, Mutex};

    use super::{
        forward_output, output_event, EventDispatcher, InputQueue, StartupGate, INPUT_CHUNK_SIZE,
        INPUT_QUEUE_CAPACITY, OUTPUT_CHUNK_SIZE,
    };
    use crate::terminal::manager::EventSink;
    use crate::terminal::protocol::{TerminalCommandError, TerminalEvent};
    use base64::Engine;

    #[derive(Default)]
    struct RecordingSink {
        chunks: Mutex<Vec<Vec<u8>>>,
        fail_after: Option<usize>,
    }

    #[derive(Default)]
    struct EventRecordingSink {
        events: Mutex<Vec<TerminalEvent>>,
    }

    #[derive(Default)]
    struct BlockingEventSink {
        state: Mutex<BlockingEventState>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct BlockingEventState {
        events: Vec<TerminalEvent>,
        output_entered: bool,
        release_output: bool,
    }

    impl BlockingEventSink {
        fn wait_for_output(&self) {
            let mut state = self.state.lock().expect("blocking sink mutex poisoned");
            while !state.output_entered {
                state = self
                    .changed
                    .wait(state)
                    .expect("blocking sink mutex poisoned");
            }
        }

        fn release_output(&self) {
            self.state
                .lock()
                .expect("blocking sink mutex poisoned")
                .release_output = true;
            self.changed.notify_all();
        }
    }

    impl EventSink for BlockingEventSink {
        fn send(&self, event: TerminalEvent) -> Result<(), TerminalCommandError> {
            let mut state = self.state.lock().expect("blocking sink mutex poisoned");
            let should_block = matches!(event, TerminalEvent::Output { .. });
            state.events.push(event);
            if should_block {
                state.output_entered = true;
                self.changed.notify_all();
                while !state.release_output {
                    state = self
                        .changed
                        .wait(state)
                        .expect("blocking sink mutex poisoned");
                }
            }
            Ok(())
        }
    }

    impl EventSink for EventRecordingSink {
        fn send(&self, event: TerminalEvent) -> Result<(), TerminalCommandError> {
            self.events
                .lock()
                .expect("recording events mutex poisoned")
                .push(event);
            Ok(())
        }
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

    #[test]
    fn input_queue_rejects_oversized_and_excess_input_without_blocking() {
        let (sender, _receiver) = sync_channel(INPUT_QUEUE_CAPACITY);
        let queue = InputQueue::new(sender);

        let oversized = queue
            .enqueue(&vec![b'x'; INPUT_CHUNK_SIZE + 1])
            .unwrap_err();
        assert_eq!(oversized.code, "terminal.input_too_large");

        for _ in 0..INPUT_QUEUE_CAPACITY {
            queue.enqueue(b"x").expect("bounded input should enqueue");
        }
        let full = queue.enqueue(b"x").unwrap_err();
        assert_eq!(full.code, "terminal.input_backpressure");

        queue.close();
        let closed = queue.enqueue(b"x").unwrap_err();
        assert_eq!(closed.code, "terminal.session_closing");
    }

    #[test]
    fn event_dispatcher_never_sends_output_after_exit() {
        let sink = Arc::new(EventRecordingSink::default());
        let dispatcher = EventDispatcher::new(sink.clone());

        dispatcher.send_output(b"before").unwrap();
        dispatcher.send_exit(Some(0), None).unwrap();
        dispatcher.send_output(b"after").unwrap();
        dispatcher
            .send_error("terminal.test", "late test error")
            .unwrap();

        let events = sink.events.lock().expect("recording events mutex poisoned");
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], TerminalEvent::Output { .. }));
        assert!(matches!(
            events[1],
            TerminalEvent::Exit { code: Some(0), .. }
        ));
    }

    #[test]
    fn exit_waits_for_in_flight_output_and_closes_the_event_gate() {
        let sink = Arc::new(BlockingEventSink::default());
        let dispatcher = Arc::new(EventDispatcher::new(sink.clone()));

        let output_dispatcher = dispatcher.clone();
        let output = std::thread::spawn(move || output_dispatcher.send_output(b"tail"));
        sink.wait_for_output();

        let exit_dispatcher = dispatcher.clone();
        let (started_sender, started_receiver) = sync_channel(0);
        let (finished_sender, finished_receiver) = sync_channel(1);
        let exit = std::thread::spawn(move || {
            started_sender.send(()).unwrap();
            let result = exit_dispatcher.send_exit(Some(0), None);
            finished_sender.send(()).unwrap();
            result
        });
        started_receiver.recv().unwrap();
        assert!(finished_receiver.try_recv().is_err());

        sink.release_output();
        output.join().unwrap().unwrap();
        exit.join().unwrap().unwrap();
        finished_receiver.recv().unwrap();
        dispatcher.send_output(b"late").unwrap();

        let state = sink.state.lock().expect("blocking sink mutex poisoned");
        assert_eq!(state.events.len(), 2);
        assert!(matches!(state.events[0], TerminalEvent::Output { .. }));
        assert!(matches!(state.events[1], TerminalEvent::Exit { .. }));
    }

    #[test]
    fn aborted_startup_gate_releases_workers_without_committing() {
        let gate = Arc::new(StartupGate::default());
        let worker_gate = gate.clone();
        let (entered_sender, entered_receiver) = sync_channel(0);
        let worker = std::thread::spawn(move || {
            entered_sender.send(()).unwrap();
            worker_gate.wait_for_commit()
        });

        entered_receiver.recv().unwrap();
        gate.abort();
        assert!(!worker.join().unwrap());
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
