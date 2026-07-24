use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};

use uuid::Uuid;

use super::protocol::{
    CreateTerminalRequest, TerminalCommandError, TerminalDescriptor, TerminalEvent,
};
use super::shell::{resolve_default_shell, validate_launch_directory, ShellSpec};

pub trait EventSink: Send + Sync {
    fn send(&self, event: TerminalEvent) -> Result<(), TerminalCommandError>;
}

pub trait PtyHandle: Send + Sync {
    fn write(&self, data: &[u8]) -> Result<(), TerminalCommandError>;
    fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalCommandError>;
    fn terminate(&self) -> Result<(), TerminalCommandError>;
}

pub trait PtySpawner: Send + Sync {
    fn spawn(
        &self,
        request: &CreateTerminalRequest,
        shell: &ShellSpec,
        sink: Arc<dyn EventSink>,
    ) -> Result<Arc<dyn PtyHandle>, TerminalCommandError>;
}

#[derive(Clone)]
pub struct TerminalManager {
    inner: Arc<TerminalManagerInner>,
}

struct TerminalManagerInner {
    spawner: Arc<dyn PtySpawner>,
    state: Mutex<ManagerState>,
    state_changed: Condvar,
}

#[derive(Default)]
struct ManagerState {
    sessions: HashMap<String, Arc<SessionRecord>>,
    cleaning: Option<Arc<CleanupAttempt>>,
    active_creates: usize,
}

struct SessionRecord {
    conversation_id: String,
    handle: Arc<dyn PtyHandle>,
    lifecycle: Mutex<SessionLifecycle>,
    lifecycle_changed: Condvar,
}

struct SessionLifecycle {
    phase: SessionPhase,
    active_operations: usize,
}

enum SessionPhase {
    Open,
    Closing(Arc<CleanupAttempt>),
    CleanupPending,
    Closed,
}

#[derive(Default)]
struct CleanupAttempt {
    state: Mutex<CleanupAttemptState>,
    finished: Condvar,
}

#[derive(Default)]
struct CleanupAttemptState {
    result: Option<Result<(), TerminalCommandError>>,
    waiters: usize,
}

impl CleanupAttempt {
    fn complete(&self, result: Result<(), TerminalCommandError>) {
        self.state
            .lock()
            .expect("terminal cleanup result mutex poisoned")
            .result = Some(result);
        self.finished.notify_all();
    }

    fn wait(&self) -> Result<(), TerminalCommandError> {
        let mut state = self
            .state
            .lock()
            .expect("terminal cleanup result mutex poisoned");
        state.waiters += 1;
        self.finished.notify_all();
        while state.result.is_none() {
            state = self
                .finished
                .wait(state)
                .expect("terminal cleanup result mutex poisoned");
        }
        state.waiters -= 1;
        state
            .result
            .as_ref()
            .expect("terminal cleanup completed without a result")
            .clone()
    }

    #[cfg(test)]
    fn wait_for_waiters(&self, expected: usize) {
        let mut state = self
            .state
            .lock()
            .expect("terminal cleanup result mutex poisoned");
        while state.waiters < expected {
            state = self
                .finished
                .wait(state)
                .expect("terminal cleanup result mutex poisoned");
        }
    }
}

impl SessionRecord {
    fn new(conversation_id: String, handle: Arc<dyn PtyHandle>) -> Self {
        Self {
            conversation_id,
            handle,
            lifecycle: Mutex::new(SessionLifecycle {
                phase: SessionPhase::Open,
                active_operations: 0,
            }),
            lifecycle_changed: Condvar::new(),
        }
    }
}

impl TerminalManager {
    pub fn new(spawner: Arc<dyn PtySpawner>) -> Self {
        Self {
            inner: Arc::new(TerminalManagerInner {
                spawner,
                state: Mutex::new(ManagerState::default()),
                state_changed: Condvar::new(),
            }),
        }
    }

    pub fn create(
        &self,
        request: CreateTerminalRequest,
        sink: Arc<dyn EventSink>,
    ) -> Result<TerminalDescriptor, TerminalCommandError> {
        let create_permit = self.begin_create()?;
        if request.conversation_id.trim().is_empty() {
            return Err(TerminalCommandError::new(
                "terminal.conversation_required",
                "A conversation is required to create a terminal session",
            ));
        }
        validate_dimensions(request.cols, request.rows)?;

        let launch_directory = validate_launch_directory(Path::new(&request.cwd))?;
        let shell = resolve_default_shell()?;
        let canonical_cwd = launch_directory.to_string_lossy().into_owned();
        let spawn_request = CreateTerminalRequest {
            cwd: canonical_cwd.clone(),
            ..request
        };
        let handle = self.inner.spawner.spawn(&spawn_request, &shell, sink)?;
        let (accepted, session_id) = create_permit.complete(Arc::new(SessionRecord::new(
            spawn_request.conversation_id,
            handle,
        )));
        if !accepted {
            return Err(manager_cleaning_error());
        }

        Ok(TerminalDescriptor {
            session_id,
            shell_name: shell.display_name,
            cwd: canonical_cwd,
        })
    }

    pub fn write(
        &self,
        conversation_id: &str,
        session_id: &str,
        data: &[u8],
    ) -> Result<(), TerminalCommandError> {
        let operation = self.begin_operation(conversation_id, session_id)?;
        operation.record.handle.write(data)
    }

    pub fn resize(
        &self,
        conversation_id: &str,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), TerminalCommandError> {
        self.ensure_not_cleaning()?;
        validate_dimensions(cols, rows)?;
        let operation = self.begin_operation(conversation_id, session_id)?;
        operation.record.handle.resize(cols, rows)
    }

    pub fn close(
        &self,
        conversation_id: &str,
        session_id: &str,
    ) -> Result<(), TerminalCommandError> {
        let record = {
            let state = self
                .inner
                .state
                .lock()
                .expect("terminal manager state mutex poisoned");
            let Some(record) = state.sessions.get(session_id) else {
                return Ok(());
            };
            Self::validate_ownership(record, conversation_id)?;
            record.clone()
        };
        self.close_record(session_id, record)
    }

    pub fn close_all(&self) -> Result<(), TerminalCommandError> {
        let (attempt, sessions) = {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("terminal manager state mutex poisoned");
            if let Some(attempt) = state.cleaning.clone() {
                drop(state);
                return attempt.wait();
            }

            let attempt = Arc::new(CleanupAttempt::default());
            state.cleaning = Some(attempt.clone());
            self.inner.state_changed.notify_all();
            while state.active_creates > 0 {
                state = self
                    .inner
                    .state_changed
                    .wait(state)
                    .expect("terminal manager state mutex poisoned");
            }
            let sessions = state
                .sessions
                .iter()
                .map(|(session_id, record)| (session_id.clone(), record.clone()))
                .collect::<Vec<_>>();
            (attempt, sessions)
        };

        let mut first_error = None;
        for (session_id, record) in sessions {
            if let Err(error) = self.close_record(&session_id, record) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        let result = first_error.map_or(Ok(()), Err);

        attempt.complete(result.clone());
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("terminal manager state mutex poisoned");
            if state
                .cleaning
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &attempt))
            {
                state.cleaning = None;
            }
        }
        self.inner.state_changed.notify_all();
        result
    }

    fn begin_create(&self) -> Result<CreatePermit, TerminalCommandError> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned");
        if state.cleaning.is_some() {
            return Err(manager_cleaning_error());
        }
        state.active_creates += 1;
        Ok(CreatePermit {
            inner: self.inner.clone(),
            completed: false,
        })
    }

    fn ensure_not_cleaning(&self) -> Result<(), TerminalCommandError> {
        let state = self
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned");
        if state.cleaning.is_some() {
            return Err(manager_cleaning_error());
        }
        Ok(())
    }

    fn begin_operation(
        &self,
        conversation_id: &str,
        session_id: &str,
    ) -> Result<OperationLease, TerminalCommandError> {
        let state = self
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned");
        if state.cleaning.is_some() {
            return Err(manager_cleaning_error());
        }
        let record = state.sessions.get(session_id).ok_or_else(|| {
            TerminalCommandError::new(
                "terminal.session_not_found",
                "Terminal session was not found",
            )
        })?;
        Self::validate_ownership(record, conversation_id)?;

        let mut lifecycle = record
            .lifecycle
            .lock()
            .expect("terminal session lifecycle mutex poisoned");
        if !matches!(lifecycle.phase, SessionPhase::Open) {
            return Err(session_closing_error());
        }
        lifecycle.active_operations += 1;
        drop(lifecycle);
        Ok(OperationLease {
            record: record.clone(),
        })
    }

    fn validate_ownership(
        record: &SessionRecord,
        conversation_id: &str,
    ) -> Result<(), TerminalCommandError> {
        if record.conversation_id != conversation_id {
            return Err(TerminalCommandError::new(
                "terminal.session_forbidden",
                "Terminal session belongs to another conversation",
            ));
        }
        Ok(())
    }

    fn close_record(
        &self,
        session_id: &str,
        record: Arc<SessionRecord>,
    ) -> Result<(), TerminalCommandError> {
        let attempt = {
            let mut lifecycle = record
                .lifecycle
                .lock()
                .expect("terminal session lifecycle mutex poisoned");
            match &lifecycle.phase {
                SessionPhase::Closing(attempt) => {
                    let attempt = attempt.clone();
                    drop(lifecycle);
                    return attempt.wait();
                }
                SessionPhase::Closed => return Ok(()),
                SessionPhase::Open | SessionPhase::CleanupPending => {}
            }

            let attempt = Arc::new(CleanupAttempt::default());
            lifecycle.phase = SessionPhase::Closing(attempt.clone());
            record.lifecycle_changed.notify_all();
            while lifecycle.active_operations > 0 {
                lifecycle = record
                    .lifecycle_changed
                    .wait(lifecycle)
                    .expect("terminal session lifecycle mutex poisoned");
            }
            attempt
        };

        let result = record.handle.terminate();
        if result.is_ok() {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("terminal manager state mutex poisoned");
            if state
                .sessions
                .get(session_id)
                .is_some_and(|current| Arc::ptr_eq(current, &record))
            {
                state.sessions.remove(session_id);
            }
        }

        {
            let mut lifecycle = record
                .lifecycle
                .lock()
                .expect("terminal session lifecycle mutex poisoned");
            lifecycle.phase = if result.is_ok() {
                SessionPhase::Closed
            } else {
                SessionPhase::CleanupPending
            };
            attempt.complete(result.clone());
        }
        record.lifecycle_changed.notify_all();
        result
    }
}

struct CreatePermit {
    inner: Arc<TerminalManagerInner>,
    completed: bool,
}

impl CreatePermit {
    fn complete(mut self, record: Arc<SessionRecord>) -> (bool, String) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned");
        let session_id = loop {
            let candidate = Uuid::new_v4().to_string();
            if !state.sessions.contains_key(&candidate) {
                break candidate;
            }
        };
        let accepted = state.cleaning.is_none();
        state.sessions.insert(session_id.clone(), record);
        state.active_creates -= 1;
        self.completed = true;
        drop(state);
        self.inner.state_changed.notify_all();
        (accepted, session_id)
    }
}

impl Drop for CreatePermit {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned");
        state.active_creates -= 1;
        drop(state);
        self.inner.state_changed.notify_all();
    }
}

struct OperationLease {
    record: Arc<SessionRecord>,
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        let mut lifecycle = self
            .record
            .lifecycle
            .lock()
            .expect("terminal session lifecycle mutex poisoned");
        lifecycle.active_operations -= 1;
        drop(lifecycle);
        self.record.lifecycle_changed.notify_all();
    }
}

impl Drop for TerminalManagerInner {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .expect("terminal manager state mutex poisoned");
        let handles: Vec<_> = state
            .sessions
            .drain()
            .map(|(_, session)| session.handle.clone())
            .collect();
        let _ = terminate_all(handles);
    }
}

fn validate_dimensions(cols: u16, rows: u16) -> Result<(), TerminalCommandError> {
    if cols == 0 || rows == 0 {
        return Err(TerminalCommandError::new(
            "terminal.invalid_size",
            "Terminal columns and rows must be greater than zero",
        ));
    }
    Ok(())
}

fn manager_cleaning_error() -> TerminalCommandError {
    TerminalCommandError::new(
        "terminal.manager_cleaning",
        "Terminal sessions are being cleaned up",
    )
}

fn session_closing_error() -> TerminalCommandError {
    TerminalCommandError::new("terminal.session_closing", "Terminal session is closing")
}

fn terminate_all(
    handles: impl IntoIterator<Item = Arc<dyn PtyHandle>>,
) -> Result<(), TerminalCommandError> {
    let mut first_error = None;
    for handle in handles {
        if let Err(error) = handle.terminate() {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;

    use super::*;

    #[derive(Default)]
    struct TestGate {
        state: Mutex<TestGateState>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct TestGateState {
        entered: usize,
        released: bool,
    }

    impl TestGate {
        fn enter_and_wait(&self) {
            let mut state = self.state.lock().expect("test gate mutex poisoned");
            state.entered += 1;
            self.changed.notify_all();
            while !state.released {
                state = self.changed.wait(state).expect("test gate mutex poisoned");
            }
        }

        fn wait_until_entered(&self) {
            let mut state = self.state.lock().expect("test gate mutex poisoned");
            while state.entered == 0 {
                state = self.changed.wait(state).expect("test gate mutex poisoned");
            }
        }

        fn release(&self) {
            self.state
                .lock()
                .expect("test gate mutex poisoned")
                .released = true;
            self.changed.notify_all();
        }
    }

    #[derive(Default)]
    struct RecordingSink;

    impl EventSink for RecordingSink {
        fn send(&self, _event: TerminalEvent) -> Result<(), TerminalCommandError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeHandle {
        writes: Mutex<Vec<Vec<u8>>>,
        sizes: Mutex<Vec<(u16, u16)>>,
        terminate_count: AtomicUsize,
        fail_termination: AtomicBool,
        write_gate: Mutex<Option<Arc<TestGate>>>,
        terminate_gate: Mutex<Option<Arc<TestGate>>>,
    }

    impl FakeHandle {
        fn writes(&self) -> Vec<Vec<u8>> {
            self.writes
                .lock()
                .expect("fake writes mutex poisoned")
                .clone()
        }

        fn sizes(&self) -> Vec<(u16, u16)> {
            self.sizes
                .lock()
                .expect("fake sizes mutex poisoned")
                .clone()
        }

        fn terminate_count(&self) -> usize {
            self.terminate_count.load(Ordering::Acquire)
        }

        fn fail_termination(&self) {
            self.fail_termination.store(true, Ordering::Release);
        }

        fn allow_termination(&self) {
            self.fail_termination.store(false, Ordering::Release);
        }

        fn block_writes(&self) -> Arc<TestGate> {
            let gate = Arc::new(TestGate::default());
            *self
                .write_gate
                .lock()
                .expect("fake write gate mutex poisoned") = Some(gate.clone());
            gate
        }

        fn block_termination(&self) -> Arc<TestGate> {
            let gate = Arc::new(TestGate::default());
            *self
                .terminate_gate
                .lock()
                .expect("fake terminate gate mutex poisoned") = Some(gate.clone());
            gate
        }
    }

    impl PtyHandle for FakeHandle {
        fn write(&self, data: &[u8]) -> Result<(), TerminalCommandError> {
            if let Some(gate) = self
                .write_gate
                .lock()
                .expect("fake write gate mutex poisoned")
                .clone()
            {
                gate.enter_and_wait();
            }
            self.writes
                .lock()
                .expect("fake writes mutex poisoned")
                .push(data.to_vec());
            Ok(())
        }

        fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalCommandError> {
            self.sizes
                .lock()
                .expect("fake sizes mutex poisoned")
                .push((cols, rows));
            Ok(())
        }

        fn terminate(&self) -> Result<(), TerminalCommandError> {
            self.terminate_count.fetch_add(1, Ordering::AcqRel);
            if let Some(gate) = self
                .terminate_gate
                .lock()
                .expect("fake terminate gate mutex poisoned")
                .clone()
            {
                gate.enter_and_wait();
            }
            if self.fail_termination.load(Ordering::Acquire) {
                return Err(TerminalCommandError::new(
                    "terminal.cleanup_failed",
                    "fake terminal cleanup failed",
                ));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSpawner {
        handles: Mutex<Vec<Arc<FakeHandle>>>,
        requests: Mutex<Vec<CreateTerminalRequest>>,
        shell_names: Mutex<Vec<String>>,
        spawn_gate: Mutex<Option<Arc<TestGate>>>,
    }

    impl FakeSpawner {
        fn handles(&self) -> Vec<Arc<FakeHandle>> {
            self.handles
                .lock()
                .expect("fake handles mutex poisoned")
                .clone()
        }

        fn requests(&self) -> Vec<CreateTerminalRequest> {
            self.requests
                .lock()
                .expect("fake requests mutex poisoned")
                .clone()
        }

        fn block_spawns(&self) -> Arc<TestGate> {
            let gate = Arc::new(TestGate::default());
            *self
                .spawn_gate
                .lock()
                .expect("fake spawn gate mutex poisoned") = Some(gate.clone());
            gate
        }
    }

    impl PtySpawner for FakeSpawner {
        fn spawn(
            &self,
            request: &CreateTerminalRequest,
            shell: &ShellSpec,
            _sink: Arc<dyn EventSink>,
        ) -> Result<Arc<dyn PtyHandle>, TerminalCommandError> {
            if let Some(gate) = self
                .spawn_gate
                .lock()
                .expect("fake spawn gate mutex poisoned")
                .clone()
            {
                gate.enter_and_wait();
            }
            let handle = Arc::new(FakeHandle::default());
            self.handles
                .lock()
                .expect("fake handles mutex poisoned")
                .push(handle.clone());
            self.requests
                .lock()
                .expect("fake requests mutex poisoned")
                .push(request.clone());
            self.shell_names
                .lock()
                .expect("fake shell names mutex poisoned")
                .push(shell.display_name.clone());
            Ok(handle)
        }
    }

    fn request(conversation_id: &str) -> CreateTerminalRequest {
        CreateTerminalRequest {
            conversation_id: conversation_id.to_string(),
            cwd: std::env::current_dir()
                .expect("current directory should resolve")
                .to_string_lossy()
                .into_owned(),
            cols: 80,
            rows: 24,
        }
    }

    fn sink() -> Arc<RecordingSink> {
        Arc::new(RecordingSink)
    }

    fn wait_until_session_closing(
        manager: &TerminalManager,
        session_id: &str,
    ) -> Arc<CleanupAttempt> {
        let record = manager
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned")
            .sessions
            .get(session_id)
            .expect("terminal session should exist")
            .clone();
        let mut lifecycle = record
            .lifecycle
            .lock()
            .expect("terminal session lifecycle mutex poisoned");
        loop {
            if let SessionPhase::Closing(attempt) = &lifecycle.phase {
                return attempt.clone();
            }
            lifecycle = record
                .lifecycle_changed
                .wait(lifecycle)
                .expect("terminal session lifecycle mutex poisoned");
        }
    }

    fn wait_until_global_cleaning(manager: &TerminalManager) -> Arc<CleanupAttempt> {
        let mut state = manager
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned");
        loop {
            if let Some(attempt) = &state.cleaning {
                return attempt.clone();
            }
            state = manager
                .inner
                .state_changed
                .wait(state)
                .expect("terminal manager state mutex poisoned");
        }
    }

    #[test]
    fn rejects_cross_conversation_input() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        let error = manager
            .write("chat-b", &descriptor.session_id, b"pwd\r")
            .unwrap_err();
        assert_eq!(error.code, "terminal.session_forbidden");
        assert!(spawner.handles()[0].writes().is_empty());
    }

    #[test]
    fn writes_resizes_and_closes_the_owned_session() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        manager
            .write("chat-a", &descriptor.session_id, b"pnpm test\r")
            .unwrap();
        manager
            .resize("chat-a", &descriptor.session_id, 120, 40)
            .unwrap();
        manager.close("chat-a", &descriptor.session_id).unwrap();
        let handle = &spawner.handles()[0];
        assert_eq!(handle.writes(), vec![b"pnpm test\r".to_vec()]);
        assert_eq!(handle.sizes(), vec![(120, 40)]);
        assert_eq!(handle.terminate_count(), 1);
    }

    #[test]
    fn repeated_close_is_idempotent() {
        let manager = TerminalManager::new(Arc::new(FakeSpawner::default()));
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        manager.close("chat-a", &descriptor.session_id).unwrap();
        manager.close("chat-a", &descriptor.session_id).unwrap();
    }

    #[test]
    fn rejects_cross_conversation_resize_and_close_without_mutating_session() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();

        let resize_error = manager
            .resize("chat-b", &descriptor.session_id, 100, 30)
            .unwrap_err();
        let close_error = manager.close("chat-b", &descriptor.session_id).unwrap_err();

        assert_eq!(resize_error.code, "terminal.session_forbidden");
        assert_eq!(close_error.code, "terminal.session_forbidden");
        assert!(spawner.handles()[0].sizes().is_empty());
        assert_eq!(spawner.handles()[0].terminate_count(), 0);
        manager
            .write("chat-a", &descriptor.session_id, b"still owned\r")
            .unwrap();
    }

    #[test]
    fn validates_create_and_resize_inputs_before_touching_the_pty() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());

        let mut missing_conversation = request("   ");
        let error = manager
            .create(missing_conversation.clone(), sink())
            .unwrap_err();
        assert_eq!(error.code, "terminal.conversation_required");

        missing_conversation.conversation_id = "chat-a".to_string();
        missing_conversation.cols = 0;
        let error = manager.create(missing_conversation, sink()).unwrap_err();
        assert_eq!(error.code, "terminal.invalid_size");

        let mut relative_cwd = request("chat-a");
        relative_cwd.cwd = "relative".to_string();
        let error = manager.create(relative_cwd, sink()).unwrap_err();
        assert_eq!(error.code, "terminal.cwd_not_absolute");
        assert!(spawner.handles().is_empty());

        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        let error = manager
            .resize("chat-a", &descriptor.session_id, 80, 0)
            .unwrap_err();
        assert_eq!(error.code, "terminal.invalid_size");
        assert!(spawner.handles()[0].sizes().is_empty());
    }

    #[test]
    fn passes_the_canonical_launch_directory_to_the_spawner_and_descriptor() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let original = request("chat-a");
        let expected = validate_launch_directory(Path::new(&original.cwd))
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let descriptor = manager.create(original, sink()).unwrap();

        assert_eq!(descriptor.cwd, expected);
        assert_eq!(spawner.requests()[0].cwd, descriptor.cwd);
        assert!(!descriptor.shell_name.is_empty());
    }

    #[test]
    fn isolates_multiple_sessions_and_uses_unpredictable_ids() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let first = manager.create(request("chat-a"), sink()).unwrap();
        let second = manager.create(request("chat-b"), sink()).unwrap();

        assert_ne!(first.session_id, second.session_id);
        assert_eq!(
            Uuid::parse_str(&first.session_id)
                .unwrap()
                .get_version_num(),
            4
        );
        assert_eq!(
            Uuid::parse_str(&second.session_id)
                .unwrap()
                .get_version_num(),
            4
        );
        manager
            .write("chat-a", &first.session_id, b"first\r")
            .unwrap();
        manager
            .write("chat-b", &second.session_id, b"second\r")
            .unwrap();
        assert_eq!(spawner.handles()[0].writes(), vec![b"first\r".to_vec()]);
        assert_eq!(spawner.handles()[1].writes(), vec![b"second\r".to_vec()]);
    }

    #[test]
    fn missing_session_operations_have_stable_behavior() {
        let manager = TerminalManager::new(Arc::new(FakeSpawner::default()));
        let write_error = manager.write("chat-a", "missing", b"pwd\r").unwrap_err();
        let resize_error = manager.resize("chat-a", "missing", 80, 24).unwrap_err();

        assert_eq!(write_error.code, "terminal.session_not_found");
        assert_eq!(resize_error.code, "terminal.session_not_found");
        manager.close("chat-a", "missing").unwrap();
    }

    #[test]
    fn close_all_attempts_every_handle_and_retains_failed_sessions_for_retry() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let first = manager.create(request("chat-a"), sink()).unwrap();
        let second = manager.create(request("chat-b"), sink()).unwrap();
        let handles = spawner.handles();
        handles[0].fail_termination();

        let error = manager.close_all().unwrap_err();

        assert_eq!(error.code, "terminal.cleanup_failed");
        assert_eq!(handles[0].terminate_count(), 1);
        assert_eq!(handles[1].terminate_count(), 1);
        let write_error = manager
            .write("chat-a", &first.session_id, b"cannot reopen\r")
            .unwrap_err();
        assert_eq!(write_error.code, "terminal.session_closing");
        manager.close("chat-b", &second.session_id).unwrap();

        handles[0].allow_termination();
        manager.close("chat-a", &first.session_id).unwrap();
        assert_eq!(handles[0].terminate_count(), 2);
        manager.close_all().unwrap();
    }

    #[test]
    fn close_all_waits_for_a_blocked_spawn_and_reclaims_its_handle() {
        let spawner = Arc::new(FakeSpawner::default());
        let spawn_gate = spawner.block_spawns();
        let manager = TerminalManager::new(spawner.clone());

        let create_manager = manager.clone();
        let create_thread = thread::spawn(move || create_manager.create(request("chat-a"), sink()));
        spawn_gate.wait_until_entered();

        let cleanup_manager = manager.clone();
        let cleanup_thread = thread::spawn(move || cleanup_manager.close_all());
        wait_until_global_cleaning(&manager);

        let create_error = manager.create(request("chat-b"), sink()).unwrap_err();
        assert_eq!(create_error.code, "terminal.manager_cleaning");
        let invalid_create_error = manager.create(request("   "), sink()).unwrap_err();
        assert_eq!(invalid_create_error.code, "terminal.manager_cleaning");

        spawn_gate.release();
        let raced_create_error = create_thread.join().unwrap().unwrap_err();
        assert_eq!(raced_create_error.code, "terminal.manager_cleaning");
        cleanup_thread.join().unwrap().unwrap();

        let handles = spawner.handles();
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].terminate_count(), 1);
        assert!(manager
            .inner
            .state
            .lock()
            .expect("terminal manager state mutex poisoned")
            .sessions
            .is_empty());
    }

    #[test]
    fn close_marks_the_session_closing_and_waits_for_a_blocked_write() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        let handle = spawner.handles()[0].clone();
        let write_gate = handle.block_writes();

        let write_manager = manager.clone();
        let write_session_id = descriptor.session_id.clone();
        let write_thread =
            thread::spawn(move || write_manager.write("chat-a", &write_session_id, b"blocked\r"));
        write_gate.wait_until_entered();

        let close_manager = manager.clone();
        let close_session_id = descriptor.session_id.clone();
        let close_thread = thread::spawn(move || close_manager.close("chat-a", &close_session_id));
        wait_until_session_closing(&manager, &descriptor.session_id);

        let write_error = manager
            .write("chat-a", &descriptor.session_id, b"late\r")
            .unwrap_err();
        let resize_error = manager
            .resize("chat-a", &descriptor.session_id, 100, 30)
            .unwrap_err();
        assert_eq!(write_error.code, "terminal.session_closing");
        assert_eq!(resize_error.code, "terminal.session_closing");
        assert_eq!(handle.terminate_count(), 0);

        write_gate.release();
        write_thread.join().unwrap().unwrap();
        close_thread.join().unwrap().unwrap();
        assert_eq!(handle.writes(), vec![b"blocked\r".to_vec()]);
        assert_eq!(handle.terminate_count(), 1);
    }

    #[test]
    fn repeated_close_and_close_all_share_a_blocked_termination() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        let handle = spawner.handles()[0].clone();
        let terminate_gate = handle.block_termination();

        let first_manager = manager.clone();
        let first_session_id = descriptor.session_id.clone();
        let first_close = thread::spawn(move || first_manager.close("chat-a", &first_session_id));
        terminate_gate.wait_until_entered();
        let close_attempt = wait_until_session_closing(&manager, &descriptor.session_id);

        let repeated_manager = manager.clone();
        let repeated_session_id = descriptor.session_id.clone();
        let repeated_close =
            thread::spawn(move || repeated_manager.close("chat-a", &repeated_session_id));
        close_attempt.wait_for_waiters(1);

        let cleanup_manager = manager.clone();
        let close_all = thread::spawn(move || cleanup_manager.close_all());
        wait_until_global_cleaning(&manager);
        close_attempt.wait_for_waiters(2);
        assert_eq!(handle.terminate_count(), 1);

        terminate_gate.release();
        first_close.join().unwrap().unwrap();
        repeated_close.join().unwrap().unwrap();
        close_all.join().unwrap().unwrap();
        assert_eq!(handle.terminate_count(), 1);
    }

    #[test]
    fn concurrent_close_all_waits_for_and_returns_the_same_cleanup_result() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        let handle = spawner.handles()[0].clone();
        handle.fail_termination();
        let terminate_gate = handle.block_termination();

        let first_manager = manager.clone();
        let first_cleanup = thread::spawn(move || first_manager.close_all());
        terminate_gate.wait_until_entered();
        let cleanup_attempt = wait_until_global_cleaning(&manager);

        let second_manager = manager.clone();
        let second_cleanup = thread::spawn(move || second_manager.close_all());
        cleanup_attempt.wait_for_waiters(1);

        let write_error = manager
            .write("chat-a", &descriptor.session_id, b"late\r")
            .unwrap_err();
        let resize_error = manager
            .resize("chat-a", &descriptor.session_id, 100, 30)
            .unwrap_err();
        assert_eq!(write_error.code, "terminal.manager_cleaning");
        assert_eq!(resize_error.code, "terminal.manager_cleaning");

        terminate_gate.release();
        let first_error = first_cleanup.join().unwrap().unwrap_err();
        let second_error = second_cleanup.join().unwrap().unwrap_err();
        assert_eq!(first_error.code, "terminal.cleanup_failed");
        assert_eq!(second_error.code, first_error.code);
        assert_eq!(second_error.message, first_error.message);
        assert_eq!(handle.terminate_count(), 1);

        handle.allow_termination();
        manager.close_all().unwrap();
        assert_eq!(handle.terminate_count(), 2);
    }

    #[test]
    fn only_the_final_manager_clone_drop_terminates_live_sessions() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        manager.create(request("chat-a"), sink()).unwrap();
        let clone = manager.clone();

        drop(clone);
        assert_eq!(spawner.handles()[0].terminate_count(), 0);

        drop(manager);
        assert_eq!(spawner.handles()[0].terminate_count(), 1);
    }

    #[test]
    fn final_manager_drop_retries_a_failed_session_cleanup() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let descriptor = manager.create(request("chat-a"), sink()).unwrap();
        let handle = spawner.handles()[0].clone();
        handle.fail_termination();

        let error = manager.close("chat-a", &descriptor.session_id).unwrap_err();
        assert_eq!(error.code, "terminal.cleanup_failed");
        assert_eq!(handle.terminate_count(), 1);

        drop(manager);
        assert_eq!(handle.terminate_count(), 2);
    }
}
