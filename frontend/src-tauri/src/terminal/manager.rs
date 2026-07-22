use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

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
    sessions: Mutex<HashMap<String, SessionRecord>>,
}

struct SessionRecord {
    conversation_id: String,
    handle: Arc<dyn PtyHandle>,
}

impl TerminalManager {
    pub fn new(spawner: Arc<dyn PtySpawner>) -> Self {
        Self {
            inner: Arc::new(TerminalManagerInner {
                spawner,
                sessions: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn create(
        &self,
        request: CreateTerminalRequest,
        sink: Arc<dyn EventSink>,
    ) -> Result<TerminalDescriptor, TerminalCommandError> {
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
        let session_id = Uuid::new_v4().to_string();

        self.inner
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned")
            .insert(
                session_id.clone(),
                SessionRecord {
                    conversation_id: spawn_request.conversation_id,
                    handle,
                },
            );

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
        self.owned_handle(conversation_id, session_id)?.write(data)
    }

    pub fn resize(
        &self,
        conversation_id: &str,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), TerminalCommandError> {
        validate_dimensions(cols, rows)?;
        self.owned_handle(conversation_id, session_id)?
            .resize(cols, rows)
    }

    pub fn close(
        &self,
        conversation_id: &str,
        session_id: &str,
    ) -> Result<(), TerminalCommandError> {
        let handle = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .expect("terminal sessions mutex poisoned");
            if !sessions.contains_key(session_id) {
                return Ok(());
            }
            Self::owned_session(&sessions, conversation_id, session_id)?;
            sessions
                .remove(session_id)
                .expect("owned terminal session disappeared")
                .handle
        };

        handle.terminate()
    }

    pub fn close_all(&self) -> Result<(), TerminalCommandError> {
        terminate_all(drain_sessions(&self.inner.sessions))
    }

    fn owned_handle(
        &self,
        conversation_id: &str,
        session_id: &str,
    ) -> Result<Arc<dyn PtyHandle>, TerminalCommandError> {
        let sessions = self
            .inner
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        Ok(Self::owned_session(&sessions, conversation_id, session_id)?
            .handle
            .clone())
    }

    fn owned_session<'a>(
        sessions: &'a HashMap<String, SessionRecord>,
        conversation_id: &str,
        session_id: &str,
    ) -> Result<&'a SessionRecord, TerminalCommandError> {
        let session = sessions.get(session_id).ok_or_else(|| {
            TerminalCommandError::new(
                "terminal.session_not_found",
                "Terminal session was not found",
            )
        })?;

        if session.conversation_id != conversation_id {
            return Err(TerminalCommandError::new(
                "terminal.session_forbidden",
                "Terminal session belongs to another conversation",
            ));
        }

        Ok(session)
    }
}

impl Drop for TerminalManagerInner {
    fn drop(&mut self) {
        let sessions = self
            .sessions
            .get_mut()
            .expect("terminal sessions mutex poisoned");
        let handles: Vec<_> = sessions
            .drain()
            .map(|(_, session)| session.handle)
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

fn drain_sessions(sessions: &Mutex<HashMap<String, SessionRecord>>) -> Vec<Arc<dyn PtyHandle>> {
    sessions
        .lock()
        .expect("terminal sessions mutex poisoned")
        .drain()
        .map(|(_, session)| session.handle)
        .collect()
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

    use super::*;

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
    }

    impl PtyHandle for FakeHandle {
        fn write(&self, data: &[u8]) -> Result<(), TerminalCommandError> {
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
    }

    impl PtySpawner for FakeSpawner {
        fn spawn(
            &self,
            request: &CreateTerminalRequest,
            shell: &ShellSpec,
            _sink: Arc<dyn EventSink>,
        ) -> Result<Arc<dyn PtyHandle>, TerminalCommandError> {
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
    fn close_all_attempts_every_handle_and_drains_sessions_after_an_error() {
        let spawner = Arc::new(FakeSpawner::default());
        let manager = TerminalManager::new(spawner.clone());
        let first = manager.create(request("chat-a"), sink()).unwrap();
        let second = manager.create(request("chat-b"), sink()).unwrap();
        spawner.handles()[0].fail_termination();

        let error = manager.close_all().unwrap_err();

        assert_eq!(error.code, "terminal.cleanup_failed");
        assert_eq!(spawner.handles()[0].terminate_count(), 1);
        assert_eq!(spawner.handles()[1].terminate_count(), 1);
        manager.close("chat-a", &first.session_id).unwrap();
        manager.close("chat-b", &second.session_id).unwrap();
        manager.close_all().unwrap();
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
}
