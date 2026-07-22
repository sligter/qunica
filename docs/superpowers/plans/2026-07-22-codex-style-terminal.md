# Codex-Style Local Terminal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Codex-style, multi-tab, interactive local PTY terminal dock to group and direct-chat pages in the Tauri desktop application.

**Architecture:** A long-lived React terminal runtime renders xterm.js panes and talks only through a `TerminalTransport` contract. The desktop transport uses Tauri commands plus one ordered `Channel` per session; Rust owns PTYs, validates conversation/session ownership, streams output, resizes terminals, and terminates process trees. Browser and cloud-workspace surfaces expose an unavailable state without invoking terminal IPC.

**Tech Stack:** React 19, TypeScript 5.7, Zustand 5, xterm.js 6.0, Tauri 2, Rust 2021, `portable-pty` 0.9, Vitest 4, Cargo tests.

## Global Constraints

- The terminal is a full local shell, not a workspace sandbox; it starts in the bound workspace but may access other host paths.
- Terminal sessions are isolated per conversation; Agent tool execution remains separate.
- Support multiple tabs with create, rename, switch, close, restart, collapse, maximize, and drag-resize.
- Route changes, dock collapse, and hide-to-tray must not stop PTYs.
- Real application exit, logout, conversation deletion, and tab close must terminate the relevant process trees.
- Application restart restores tab names, order, active tab, and launch directories only; it creates new shells and never persists commands, output, environment, or PTY state.
- Windows is the release gate. Keep the PTY, shell-resolution, process-tree, and transport boundaries cross-platform.
- Default dock height is 35%; minimum height is 180px; maximum height is 70% of the available main content height.
- xterm.js scrollback is 5,000 lines. PTY output chunks are at most 16 KiB and are transported as Base64 bytes.
- Terminal logs may include lifecycle state, shell kind, exit code, and cleanup result; they must not include terminal input, output, environment variables, or full command lines.

---

## File Structure

### Desktop Rust

- `frontend/src-tauri/src/terminal/mod.rs` — terminal module exports and Tauri command functions.
- `frontend/src-tauri/src/terminal/protocol.rs` — serializable requests, descriptors, events, and stable command errors.
- `frontend/src-tauri/src/terminal/shell.rs` — platform default-shell resolution and launch-directory validation.
- `frontend/src-tauri/src/terminal/manager.rs` — conversation/session ownership, lifecycle, and testable PTY abstractions.
- `frontend/src-tauri/src/terminal/native.rs` — `portable-pty` adapter, reader/wait threads, Base64 output, and shutdown behavior.
- `frontend/src-tauri/src/terminal/process_tree.rs` — Windows process-tree and Unix process-group termination.
- `frontend/src-tauri/src/main.rs` — register `TerminalManager`, commands, and exit cleanup.
- `frontend/src-tauri/Cargo.toml` / `frontend/src-tauri/Cargo.lock` — PTY and UUID dependencies.

### Frontend

- `frontend/src/terminal/types.ts` — transport and UI domain types.
- `frontend/src/terminal/transport.ts` — `TerminalTransport` interface and unavailable transport.
- `frontend/src/terminal/tauriTransport.ts` — Tauri `invoke`/`Channel` implementation.
- `frontend/src/terminal/metadataStore.ts` — versioned, non-sensitive persisted tab metadata.
- `frontend/src/terminal/TerminalRuntimeProvider.tsx` — long-lived sessions, active conversation registration, and cleanup API.
- `frontend/src/terminal/useTerminalConversationRegistration.ts` — resolve a conversation workspace and register availability with the runtime.
- `frontend/src/terminal/TerminalDock.tsx` — dock chrome, tabs, empty/error states, safety notice, collapse, and maximize.
- `frontend/src/terminal/TerminalPane.tsx` — xterm.js instance, input/output, fit, focus, and resize.
- `frontend/src/terminal/usePersistentPaneHeight.ts` — pointer/keyboard dock height behavior.
- `frontend/src/components/layout/AppLayout.tsx` — mount the provider and dock outside routed pages.
- `frontend/src/components/chat/ConversationChatView.tsx` — register the active conversation/workspace and add the header toggle.
- `frontend/src/pages/group/GroupChatPage.tsx` / `frontend/src/pages/chat/DirectChatPage.tsx` — pass `workspaceId`.
- `frontend/src/components/layout/AppSidebar.tsx` / `frontend/src/pages/group/GroupSettingsTab.tsx` — close sessions on delete and logout.
- `frontend/src/i18n/resources/en-US.ts` / `frontend/src/i18n/resources/zh-CN.ts` — all terminal-facing copy.
- `frontend/src/index.css` — terminal theme tokens and dock styling hooks.
- `frontend/package.json` / `pnpm-lock.yaml` — xterm.js packages.

---

### Task 1: Desktop Terminal Protocol and Shell Resolution

**Files:**
- Modify: `frontend/src-tauri/Cargo.toml`
- Modify: `frontend/src-tauri/src/main.rs:1`
- Create: `frontend/src-tauri/src/terminal/mod.rs`
- Create: `frontend/src-tauri/src/terminal/protocol.rs`
- Create: `frontend/src-tauri/src/terminal/shell.rs`
- Test: `frontend/src-tauri/src/terminal/shell.rs`

**Interfaces:**
- Produces: `CreateTerminalRequest`, `TerminalDescriptor`, `TerminalEvent`, `TerminalCommandError`, `ShellSpec`, `validate_launch_directory(Path)`, and `resolve_default_shell()`.
- Consumes: no feature-local interfaces.

- [ ] **Step 1: Add the failing shell-resolution and path-validation tests**

Add `mod terminal;` near the other module declarations in `frontend/src-tauri/src/main.rs`. Create `terminal/mod.rs` with `pub mod protocol; pub mod shell;`. Create `terminal/shell.rs` with these tests first:

```rust
#[cfg(test)]
mod tests {
    use super::{resolve_shell_with, validate_launch_directory};
    use std::{collections::HashMap, ffi::OsString, fs};

    #[test]
    fn windows_prefers_pwsh_then_powershell_then_cmd() {
        let found = HashMap::from([
            ("pwsh".to_string(), "C:/Tools/pwsh.exe".into()),
            ("powershell".to_string(), "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe".into()),
            ("cmd".to_string(), "C:/Windows/System32/cmd.exe".into()),
        ]);
        let shell = resolve_shell_with(true, None, |name| {
            found.get(&name.to_string_lossy().to_string()).cloned()
        })
        .expect("pwsh should resolve");
        assert_eq!(shell.display_name, "PowerShell");
        assert!(shell.program.ends_with("pwsh.exe"));
    }

    #[test]
    fn unix_prefers_valid_shell_environment() {
        let shell = resolve_shell_with(false, Some(OsString::from("/opt/bin/fish")), |name| {
            (name == "/opt/bin/fish").then(|| "/opt/bin/fish".into())
        })
        .expect("SHELL should resolve");
        assert_eq!(shell.display_name, "fish");
    }

    #[test]
    fn launch_directory_must_be_an_existing_absolute_directory() {
        let root = std::env::temp_dir().join(format!("ag-swarmer-terminal-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create temp directory");
        assert_eq!(validate_launch_directory(&root).unwrap(), root);
        assert!(validate_launch_directory(std::path::Path::new("relative")).is_err());
        fs::remove_dir_all(&root).expect("remove temp directory");
    }
}
```

- [ ] **Step 2: Run the focused Rust test and verify it fails**

Run:

```powershell
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::shell::tests -- --nocapture
```

Expected: compilation fails because `resolve_shell_with`, `validate_launch_directory`, and `ShellSpec` do not exist.

- [ ] **Step 3: Add exact protocol types and shell resolution**

Add these dependencies:

```toml
portable-pty = "0.9.0"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }

[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

Create `terminal/protocol.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalRequest {
    pub conversation_id: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDescriptor {
    pub session_id: String,
    pub shell_name: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TerminalEvent {
    Output { bytes_base64: String },
    Exit { code: Option<u32>, signal: Option<String> },
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCommandError {
    pub code: String,
    pub message: String,
}

impl TerminalCommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}
```

Implement `terminal/shell.rs` with `ShellSpec { program: PathBuf, display_name: String }`, absolute-directory validation, PATH lookup, Windows priority `pwsh -> powershell -> cmd`, and Unix `$SHELL -> /bin/zsh -> /bin/bash -> /bin/sh`. Use the injectable signature tested above:

```rust
pub(crate) fn resolve_shell_with(
    windows: bool,
    shell_env: Option<OsString>,
    mut locate: impl FnMut(&OsStr) -> Option<PathBuf>,
) -> Result<ShellSpec, TerminalCommandError>
```

`resolve_default_shell()` must call this helper with `cfg!(windows)`, `std::env::var_os("SHELL")`, and a PATH-searching locator. `validate_launch_directory` must canonicalize the directory and return error codes `terminal.cwd_not_absolute`, `terminal.cwd_missing`, or `terminal.cwd_not_directory`.

- [ ] **Step 4: Run focused and crate tests**

Run:

```powershell
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::shell::tests -- --nocapture
cargo test --manifest-path frontend/src-tauri/Cargo.toml
```

Expected: all shell tests and existing desktop tests pass; `Cargo.lock` records `portable-pty` and `uuid`.

- [ ] **Step 5: Commit the protocol and resolver**

```powershell
git add frontend/src-tauri/Cargo.toml frontend/src-tauri/Cargo.lock frontend/src-tauri/src/main.rs frontend/src-tauri/src/terminal
git commit -m "feat(terminal): define desktop protocol and shell resolution"
```

---

### Task 2: Testable Terminal Manager and Conversation Isolation

**Files:**
- Modify: `frontend/src-tauri/src/terminal/mod.rs`
- Create: `frontend/src-tauri/src/terminal/manager.rs`
- Test: `frontend/src-tauri/src/terminal/manager.rs`

**Interfaces:**
- Consumes: `CreateTerminalRequest`, `TerminalDescriptor`, `TerminalEvent`, `TerminalCommandError`, `ShellSpec`.
- Produces: `EventSink`, `PtyHandle`, `PtySpawner`, `TerminalManager::create`, `write`, `resize`, `close`, and `close_all`.

- [ ] **Step 1: Write failing ownership and lifecycle tests with a fake PTY**

Define fake implementations inside `manager.rs` tests and cover these exact cases:

```rust
#[test]
fn rejects_cross_conversation_input() {
    let spawner = Arc::new(FakeSpawner::default());
    let manager = TerminalManager::new(spawner.clone());
    let descriptor = manager.create(request("chat-a"), Arc::new(RecordingSink::default())).unwrap();
    let error = manager.write("chat-b", &descriptor.session_id, b"pwd\r").unwrap_err();
    assert_eq!(error.code, "terminal.session_forbidden");
    assert!(spawner.handles()[0].writes().is_empty());
}

#[test]
fn writes_resizes_and_closes_the_owned_session() {
    let spawner = Arc::new(FakeSpawner::default());
    let manager = TerminalManager::new(spawner.clone());
    let descriptor = manager.create(request("chat-a"), Arc::new(RecordingSink::default())).unwrap();
    manager.write("chat-a", &descriptor.session_id, b"pnpm test\r").unwrap();
    manager.resize("chat-a", &descriptor.session_id, 120, 40).unwrap();
    manager.close("chat-a", &descriptor.session_id).unwrap();
    let handle = &spawner.handles()[0];
    assert_eq!(handle.writes(), vec![b"pnpm test\r".to_vec()]);
    assert_eq!(handle.sizes(), vec![(120, 40)]);
    assert_eq!(handle.terminate_count(), 1);
}

#[test]
fn repeated_close_is_idempotent() {
    let manager = TerminalManager::new(Arc::new(FakeSpawner::default()));
    let descriptor = manager.create(request("chat-a"), Arc::new(RecordingSink::default())).unwrap();
    manager.close("chat-a", &descriptor.session_id).unwrap();
    manager.close("chat-a", &descriptor.session_id).unwrap();
}
```

The fake `PtyHandle` must record byte writes, `(cols, rows)` sizes, and terminate count behind `Mutex` values; `FakeSpawner` must return a descriptor with a deterministic shell name and retain handles for assertions.

- [ ] **Step 2: Run the manager tests and verify they fail**

```powershell
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::manager::tests -- --nocapture
```

Expected: compilation fails because manager traits and methods are missing.

- [ ] **Step 3: Implement the manager boundary**

Create these public traits and manager shape:

```rust
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
```

`create` validates the conversation ID, launch directory, dimensions (`cols` and `rows` must be non-zero), resolves the shell, creates a UUID v4 `sessionId`, calls the spawner, and inserts the record. `write`, `resize`, and `close` use one private `owned_session(conversation_id, session_id)` check. A missing already-closed session returns success only for `close`; a present session owned by another conversation returns `terminal.session_forbidden`. `close_all` drains the map and calls `terminate` on every handle, returning the first cleanup error after attempting all handles.

Implement `Drop` on `TerminalManagerInner`, not on the cloneable `TerminalManager`; the final `Arc` drop drains and terminates remaining handles. This prevents temporary manager clones used by `spawn_blocking` from closing live sessions.

- [ ] **Step 4: Run manager tests and formatting**

```powershell
cargo fmt --manifest-path frontend/src-tauri/Cargo.toml -- --check
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::manager::tests -- --nocapture
```

Expected: all manager tests pass and formatting reports no diff.

- [ ] **Step 5: Commit the manager**

```powershell
git add frontend/src-tauri/src/terminal
git commit -m "feat(terminal): manage isolated terminal sessions"
```

---

### Task 3: Native PTY Adapter, Process-Tree Cleanup, and Tauri Commands

**Files:**
- Modify: `frontend/src-tauri/src/terminal/mod.rs`
- Modify: `frontend/src-tauri/src/main.rs:1-490`
- Create: `frontend/src-tauri/src/terminal/native.rs`
- Create: `frontend/src-tauri/src/terminal/process_tree.rs`
- Test: `frontend/src-tauri/src/terminal/native.rs`
- Test: `frontend/src-tauri/src/terminal/process_tree.rs`

**Interfaces:**
- Consumes: manager traits and terminal protocol from Tasks 1–2.
- Produces: `NativePtySpawner`, `TauriChannelSink`, five registered Tauri commands, and exit cleanup.

- [ ] **Step 1: Write failing process-tree and output-chunk tests**

Add platform-independent argument and encoding tests:

```rust
#[test]
fn windows_taskkill_targets_the_process_tree() {
    assert_eq!(
        taskkill_args(4242),
        ["/PID", "4242", "/T", "/F"].map(str::to_string),
    );
}

#[test]
fn output_chunks_round_trip_as_bytes() {
    let bytes = [0xf0, 0x9f, 0x98, 0x80, b'\r', b'\n'];
    let event = output_event(&bytes);
    let TerminalEvent::Output { bytes_base64 } = event else { panic!("expected output") };
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(bytes_base64).unwrap(), bytes);
}
```

Add a `#[cfg(windows)] #[ignore = "manual Windows PTY smoke test"]` test that spawns PowerShell in a temporary directory, writes `Write-Output PTY_OK\r`, waits for `PTY_OK` through a recording sink, resizes to `100x30`, starts a long-lived descendant PowerShell process and captures its PID, then terminates the terminal and asserts that both the shell and descendant PID no longer exist. This ignored test is part of the Windows release gate command in Task 8.

- [ ] **Step 2: Run the focused tests and verify they fail**

```powershell
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::process_tree::tests -- --nocapture
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::native::tests -- --nocapture
```

Expected: compilation fails because `taskkill_args`, `output_event`, and `NativePtySpawner` are missing.

- [ ] **Step 3: Implement `portable-pty` spawning and bounded streaming**

`NativePtySpawner::spawn` must:

1. Open `native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })`.
2. Build the resolved program with `CommandBuilder`, set `cwd`, and spawn on the slave.
3. Drop the slave, clone the master reader, and take the master writer once.
4. Start a named reader thread using a `[u8; 16 * 1024]` buffer. Encode each non-empty read into `TerminalEvent::Output` without logging its content.
5. Start a wait thread that emits `TerminalEvent::Exit` with `ExitStatus::exit_code()` and a signal string when present.
6. Store the master, writer, cloned child killer, PID, and an `AtomicBool` exit marker in `NativePtyHandle`.

Emit structured `tracing` lifecycle fields for create, shell kind, exit code, close reason, and forced-cleanup result. Never include `data`, encoded output, environment values, or the full command line in a trace field.

Implement `write` with `write_all` plus `flush`, `resize` with `PtySize`, and `terminate` as:

```rust
fn terminate(&self) -> Result<(), TerminalCommandError> {
    if self.exited.load(Ordering::Acquire) {
        return Ok(());
    }
    self.write_graceful_exit();
    for _ in 0..20 {
        if self.exited.load(Ordering::Acquire) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    terminate_process_tree(self.pid)?;
    let _ = self.killer.lock().expect("terminal killer mutex poisoned").kill();
    Ok(())
}
```

On Windows, `terminate_process_tree` runs `taskkill /PID <pid> /T /F` and treats “process not found” as success. On Unix, use the PTY session leader/process group and send `SIGTERM`, wait 500ms, then `SIGKILL`; keep this implementation behind `#[cfg(unix)]` so Windows remains the release gate.

- [ ] **Step 4: Add the Tauri channel sink and commands**

Implement `TauriChannelSink(Channel<TerminalEvent>)` as `EventSink`. Add commands in `terminal/mod.rs` with these exact signatures:

```rust
#[tauri::command]
pub fn terminal_create(
    request: CreateTerminalRequest,
    on_event: Channel<TerminalEvent>,
    manager: State<'_, TerminalManager>,
) -> Result<TerminalDescriptor, TerminalCommandError>;

#[tauri::command]
pub fn terminal_write(
    conversation_id: String,
    session_id: String,
    data: String,
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError>;

#[tauri::command]
pub fn terminal_resize(
    conversation_id: String,
    session_id: String,
    cols: u16,
    rows: u16,
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError>;

#[tauri::command]
pub async fn terminal_close(
    conversation_id: String,
    session_id: String,
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError>;

#[tauri::command]
pub async fn terminal_close_all(
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError>;
```

Run blocking close work via `tauri::async_runtime::spawn_blocking`. In `main.rs`, manage `TerminalManager::new(Arc::new(NativePtySpawner))`, register all five commands, call `close_all` before the tray exit path shuts down the backend, and keep `Drop` cleanup as a final safeguard. Do not call cleanup when the main window is merely hidden.

- [ ] **Step 5: Verify native Rust and desktop registration**

```powershell
cargo fmt --manifest-path frontend/src-tauri/Cargo.toml -- --check
cargo test --manifest-path frontend/src-tauri/Cargo.toml
cargo clippy --manifest-path frontend/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all desktop tests pass; Clippy reports no warnings; the ignored real PTY test is listed but not run.

- [ ] **Step 6: Commit the native desktop bridge**

```powershell
git add frontend/src-tauri/src/main.rs frontend/src-tauri/src/terminal
git commit -m "feat(terminal): bridge native PTY sessions through Tauri"
```

---

### Task 4: Frontend Transport Contract and Persisted Metadata

**Files:**
- Modify: `frontend/package.json`
- Modify: `pnpm-lock.yaml`
- Create: `frontend/src/terminal/types.ts`
- Create: `frontend/src/terminal/transport.ts`
- Create: `frontend/src/terminal/tauriTransport.ts`
- Create: `frontend/src/terminal/metadataStore.ts`
- Test: `frontend/src/terminal/tauriTransport.test.ts`
- Test: `frontend/src/terminal/metadataStore.test.ts`

**Interfaces:**
- Consumes: Tauri command names and payloads from Task 3.
- Produces: `TerminalTransport`, `createTauriTerminalTransport`, `TerminalMetadataStore`, and shared UI types.

- [ ] **Step 1: Install pinned-compatible xterm packages**

```powershell
pnpm --filter @ag-swarmer/frontend add @xterm/xterm@^6.0.0 @xterm/addon-fit@^0.11.0
```

Expected: `frontend/package.json` and `pnpm-lock.yaml` contain both packages and no unrelated dependency changes.

- [ ] **Step 2: Write failing metadata persistence tests**

Test a pure `loadTerminalMetadata`, `saveTerminalMetadata`, and `clearTerminalMetadata` API:

```ts
it('persists only restorable tab metadata', () => {
  saveTerminalMetadata({
    height: 320,
    conversations: {
      'chat-1': {
        open: true,
        activeTabId: 'tab-1',
        tabs: [{ id: 'tab-1', label: 'PowerShell', launchDirectory: 'D:/project' }],
      },
    },
  })
  const raw = localStorage.getItem('ag-swarmer:terminal-metadata:v1') ?? ''
  expect(raw).toContain('PowerShell')
  expect(raw).not.toContain('sessionId')
  expect(raw).not.toContain('output')
  expect(loadTerminalMetadata().conversations['chat-1']?.activeTabId).toBe('tab-1')
})

it('falls back to an empty schema for invalid storage', () => {
  localStorage.setItem('ag-swarmer:terminal-metadata:v1', '{broken')
  expect(loadTerminalMetadata()).toEqual({ height: 0, conversations: {} })
})
```

- [ ] **Step 3: Write failing Tauri transport contract tests**

Mock `@tauri-apps/api/core` and assert exact command names and payload keys:

```ts
it('creates a session with an ordered channel callback', async () => {
  const onEvent = vi.fn()
  invokeMock.mockResolvedValue({ sessionId: 'session-1', shellName: 'PowerShell', cwd: 'D:/project' })
  const transport = createTauriTerminalTransport()
  const descriptor = await transport.create(
    { conversationId: 'chat-1', cwd: 'D:/project', cols: 80, rows: 24 },
    onEvent,
  )
  expect(descriptor.sessionId).toBe('session-1')
  expect(invokeMock).toHaveBeenCalledWith('terminal_create', expect.objectContaining({
    request: { conversationId: 'chat-1', cwd: 'D:/project', cols: 80, rows: 24 },
    onEvent: expect.any(ChannelMock),
  }))
})
```

Also test `write`, `resize`, `close`, `closeAll`, Rust error-object normalization, and Base64 output decoding into `Uint8Array`.

- [ ] **Step 4: Implement types, persistence, and transport**

Define the contract exactly:

```ts
export type TerminalConversationTarget =
  | { conversationId: string; availability: 'ready'; cwd: string }
  | { conversationId: string; availability: 'loading' }
  | { conversationId: string; availability: 'desktopRequired' }
  | { conversationId: string; availability: 'workspaceRequired' }
  | { conversationId: string; availability: 'localWorkspaceRequired' }
  | { conversationId: string; availability: 'pathRequired' }

export interface CreateTerminalRequest {
  conversationId: string
  cwd: string
  cols: number
  rows: number
}

export interface TerminalDescriptor {
  sessionId: string
  shellName: string
  cwd: string
}

export type TerminalEvent =
  | { event: 'output'; data: { bytes: Uint8Array } }
  | { event: 'exit'; data: { code: number | null; signal: string | null } }
  | { event: 'error'; data: { code: string; message: string } }

export interface TerminalTransport {
  create(request: CreateTerminalRequest, onEvent: (event: TerminalEvent) => void): Promise<TerminalDescriptor>
  write(conversationId: string, sessionId: string, data: string): Promise<void>
  resize(conversationId: string, sessionId: string, cols: number, rows: number): Promise<void>
  close(conversationId: string, sessionId: string): Promise<void>
  closeAll(): Promise<void>
}
```

Use a discriminated frontend event union with `output.bytes: Uint8Array`, `exit.code`, and `error.code/message`. `createTauriTerminalTransport` creates `new Channel<WireTerminalEvent>()`, sets `channel.onmessage`, decodes `bytesBase64`, and invokes the exact Rust commands. The unavailable transport rejects with `new TerminalTransportError('terminal.desktop_required', 'Terminal is available only in the desktop app.')` and never invokes Tauri IPC.

Metadata types must contain only `{height, conversations}` and `{open, activeTabId, tabs: {id,label,launchDirectory}[]}`. Validate parsed JSON field-by-field rather than trusting a type cast. Clamp stored height later in the height hook because available viewport size is not known during load.

- [ ] **Step 5: Run frontend contract tests**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/terminal/metadataStore.test.ts src/terminal/tauriTransport.test.ts
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: both files pass and TypeScript reports zero errors.

- [ ] **Step 6: Commit frontend contracts**

```powershell
git add frontend/package.json pnpm-lock.yaml frontend/src/terminal
git commit -m "feat(terminal): add frontend transport and metadata contracts"
```

---

### Task 5: Long-Lived Terminal Runtime and App Layout Host

**Files:**
- Create: `frontend/src/terminal/TerminalRuntimeProvider.tsx`
- Test: `frontend/src/terminal/TerminalRuntimeProvider.test.tsx`
- Modify: `frontend/src/components/layout/AppLayout.tsx:1-16`
- Modify: `frontend/src/components/layout/AppLayout.test.tsx`

**Interfaces:**
- Consumes: `TerminalTransport`, metadata functions, `WorkspaceRead`, and `isDesktopRuntime()`.
- Produces: `useTerminalRuntime()`, `TerminalRuntimeProvider`, and a persistent dock host contract. Workspace resolution is added in Task 7.

- [ ] **Step 1: Write failing provider lifecycle tests**

Use a fake transport with deterministic descriptors and captured event callbacks. Cover:

```tsx
it('keeps chat-a running while chat-b becomes active', async () => {
  const transport = createFakeTransport()
  const { rerender } = render(<Harness transport={transport} conversationId="chat-a" cwd="D:/a" />)
  await userEvent.click(screen.getByRole('button', { name: 'toggle-test-terminal' }))
  expect(transport.create).toHaveBeenCalledTimes(1)
  rerender(<Harness transport={transport} conversationId="chat-b" cwd="D:/b" />)
  expect(transport.close).not.toHaveBeenCalled()
})

it('restores metadata by creating fresh sessions on first activation', async () => {
  seedMetadata('chat-a', [
    { id: 'tab-a', label: 'PowerShell', launchDirectory: 'D:/a' },
    { id: 'tab-b', label: 'Dev server', launchDirectory: 'D:/missing' },
  ])
  const transport = createFakeTransport()
  render(<Harness transport={transport} conversationId="chat-a" cwd="D:/a" />)
  await waitFor(() => expect(transport.create).toHaveBeenCalledTimes(2))
  expect(transport.create).toHaveBeenNthCalledWith(2, expect.objectContaining({ cwd: 'D:/a' }), expect.any(Function))
})
```

Also test close-tab, restart, rename, select, close-conversation, close-all, exit/error events, and that output events are delivered only to the matching runtime tab.

- [ ] **Step 2: Run the provider test and verify it fails**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/terminal/TerminalRuntimeProvider.test.tsx
```

Expected: module resolution fails because the provider does not exist.

- [ ] **Step 3: Implement runtime state and registration**

Expose this context surface:

```ts
interface TerminalRuntimeContextValue {
  activeConversation: TerminalConversationTarget | null
  allTabs: TerminalRuntimeTab[]
  activeTabs: TerminalRuntimeTab[]
  activeTabId: string | null
  isDockOpen: boolean
  isMaximized: boolean
  registerConversation(target: TerminalConversationTarget): () => void
  toggleDock(): Promise<void>
  createTab(): Promise<void>
  selectTab(tabId: string): void
  renameTab(tabId: string, label: string): void
  closeTab(tabId: string): Promise<void>
  restartTab(tabId: string): Promise<void>
  closeConversation(conversationId: string, clearMetadata: boolean): Promise<void>
  closeAll(clearMetadata: boolean): Promise<void>
  toggleMaximized(): void
}

interface TerminalRuntimeTab {
  tabId: string
  conversationId: string
  sessionId: string | null
  label: string
  launchDirectory: string
  status: 'starting' | 'running' | 'exited' | 'error'
  exitCode: number | null
  error: TerminalTransportError | null
}
```

Keep runtime-only fields (`sessionId`, event callback, status, exit code, error, and queued output listeners) in provider state, not localStorage. Create all restored tabs the first time a conversation registers. Before create, try a saved launch directory only when `looksAbsolute(savedDirectory)` is true; otherwise use the current workspace root. Since the frontend cannot prove host existence, the Rust `cwd` validation remains authoritative and a failed saved directory retries once with the current workspace root.

Use a monotonic generation token per tab so late events from a closed/restarted session cannot mutate the replacement runtime. Batch React state changes from output: terminal bytes go through a tab-local subscriber set and are not copied into provider state.

Map create failures to tab status `error`; map exit events to `exited`; map write failures to `error`. Treat resize failures as non-fatal diagnostic state so the next observed size retries. `closeTab`, `closeConversation`, and `closeAll` must remove runtime entries in `finally` blocks so repeated cleanup is idempotent even if the native command reports an error.

- [ ] **Step 4: Mount provider and stable host in `AppLayout`**

Change the layout shape to:

```tsx
export function AppLayout() {
  return (
    <TerminalRuntimeProvider>
      <div className="flex h-screen min-h-0 bg-background" onContextMenu={(event) => event.preventDefault()}>
        <AppSidebar />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-hidden"><Outlet /></div>
          <div data-testid="terminal-dock-host" className="shrink-0" />
        </main>
      </div>
    </TerminalRuntimeProvider>
  )
}
```

Task 6 replaces the stable host element with the visual `TerminalDock`. Allow tests to inject a fake transport through an optional provider prop while production chooses Tauri or unavailable transport from `isDesktopRuntime()`.

- [ ] **Step 5: Run provider and layout tests**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/terminal/TerminalRuntimeProvider.test.tsx src/components/layout/AppLayout.test.tsx
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: lifecycle and existing layout tests pass; no terminal is created merely by rendering a non-chat route.

- [ ] **Step 6: Commit the runtime**

```powershell
git add frontend/src/terminal/TerminalRuntimeProvider.tsx frontend/src/terminal/TerminalRuntimeProvider.test.tsx frontend/src/components/layout/AppLayout.tsx frontend/src/components/layout/AppLayout.test.tsx
git commit -m "feat(terminal): keep terminal runtime alive across routes"
```

---

### Task 6: xterm Pane, Resizable Dock, Tabs, and Safety Notice

**Files:**
- Create: `frontend/src/terminal/TerminalPane.tsx`
- Create: `frontend/src/terminal/TerminalDock.tsx`
- Create: `frontend/src/terminal/usePersistentPaneHeight.ts`
- Test: `frontend/src/terminal/TerminalPane.test.tsx`
- Test: `frontend/src/terminal/TerminalDock.test.tsx`
- Test: `frontend/src/terminal/usePersistentPaneHeight.test.tsx`
- Modify: `frontend/src/terminal/TerminalRuntimeProvider.tsx`
- Modify: `frontend/src/components/layout/AppLayout.tsx`
- Modify: `frontend/src/index.css`

**Interfaces:**
- Consumes: provider context and runtime tab subscribers from Task 5.
- Produces: production `TerminalDock`, xterm rendering, resize coalescing, and accessible dock controls.

- [ ] **Step 1: Write failing pane-height tests**

Test the hook with a 1,000px available content height:

```tsx
it('clamps pointer resizing between 180px and 70 percent', () => {
  const { result } = renderHook(() => usePersistentPaneHeight({ availableHeight: 1000 }))
  act(() => result.current.resizeTo(50))
  expect(result.current.height).toBe(180)
  act(() => result.current.resizeTo(900))
  expect(result.current.height).toBe(700)
})

it('uses 35 percent by default and restores it on double click', () => {
  const { result } = renderHook(() => usePersistentPaneHeight({ availableHeight: 1000 }))
  expect(result.current.height).toBe(350)
  act(() => result.current.resizeTo(500))
  act(() => result.current.reset())
  expect(result.current.height).toBe(350)
})
```

Also verify ArrowUp/ArrowDown steps, pointer listener cleanup, and persisted height clamping.

- [ ] **Step 2: Write failing dock and xterm tests**

Mock `@xterm/xterm` and `@xterm/addon-fit`. Assert:

- `TerminalPane` constructs one `Terminal({ scrollback: 5000, convertEol: false })` per runtime tab.
- `onData` calls provider `write` with the active session.
- output subscribers call `terminal.write(Uint8Array)`.
- `ResizeObserver` calls `fit()`, then coalesces `resize(cols, rows)`.
- every runtime pane remains mounted while another tab or conversation is active.
- the dock exposes New, Rename, Close, Restart, Collapse, Maximize, and Restore controls with translated accessible labels.
- closing the last tab shows the empty state instead of collapsing.
- the first-use full-shell notice is stored under `ag-swarmer:terminal-full-access-warning:v1` and does not reappear after dismissal.

- [ ] **Step 3: Run UI tests and verify they fail**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/terminal/usePersistentPaneHeight.test.tsx src/terminal/TerminalPane.test.tsx src/terminal/TerminalDock.test.tsx
```

Expected: the three modules are missing.

- [ ] **Step 4: Implement `TerminalPane`**

Create xterm with:

```ts
const terminal = new Terminal({
  allowProposedApi: false,
  convertEol: false,
  cursorBlink: true,
  fontFamily: '"Cascadia Code", "JetBrains Mono", "SFMono-Regular", Consolas, monospace',
  fontSize: 13,
  scrollback: 5_000,
  theme: terminalThemeFromDocument(),
})
const fit = new FitAddon()
terminal.loadAddon(fit)
terminal.open(container)
```

Subscribe once to input, runtime bytes, and theme changes. Use a `ResizeObserver`; schedule one `requestAnimationFrame`, call `fit.fit()`, and only send resize when `cols` or `rows` changed. Dispose input, output, observer, frame, fit addon, and terminal only when the runtime tab is permanently removed—not when switching routes or active tabs.

- [ ] **Step 5: Implement the dock and height hook**

The dock must:

- keep one hidden `TerminalPane` mounted for every entry in `allTabs`, while showing dock chrome only when an active conversation is registered and metadata says open;
- use a separator with `role="separator"`, `aria-orientation="horizontal"`, and keyboard steps;
- reset to 35% when the separator is double-clicked;
- render tab buttons with status dots and inline rename on double click;
- call provider actions rather than transport methods directly;
- show an exited label with exit code and Restart;
- show an error label/message with Retry;
- show browser, missing-workspace, cloud-workspace, and missing-path unavailable states;
- use one maximize flag that sets the dock to the full available content height and restores the prior height;
- keep a 30–32px tab bar and avoid covering the Composer.

Use this persistent pane-host structure so inactive conversations keep their xterm buffers:

```tsx
<section style={{ height: chromeVisible ? displayedHeight : 0 }} aria-hidden={!chromeVisible}>
  {chromeVisible ? <TerminalTabBar /> : null}
  <div className="relative min-h-0 flex-1">
    {allTabs.map((tab) => (
      <div key={tab.tabId} hidden={tab.tabId !== activeTabId || !chromeVisible} className="absolute inset-0">
        <TerminalPane tab={tab} />
      </div>
    ))}
  </div>
</section>
```

Replace the stable host element in `AppLayout` with `<TerminalDock />`; do not conditionally unmount it on non-chat routes. Import `@xterm/xterm/css/xterm.css` once in `TerminalPane.tsx`. Add CSS variables in `index.css` for terminal background, foreground, selection, cursor, and inactive-tab colors in both themes; `terminalThemeFromDocument()` reads those computed variables.

- [ ] **Step 6: Run UI, type, and lint checks**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/terminal
pnpm --filter @ag-swarmer/frontend type-check
pnpm --filter @ag-swarmer/frontend lint
```

Expected: terminal tests pass; TypeScript and ESLint report zero errors.

- [ ] **Step 7: Commit the terminal dock**

```powershell
git add frontend/src/terminal frontend/src/components/layout/AppLayout.tsx frontend/src/index.css
git commit -m "feat(terminal): render resizable multi-tab terminal dock"
```

---

### Task 7: Chat Registration, Keyboard Shortcut, Localization, Delete, and Logout Cleanup

**Files:**
- Modify: `frontend/src/components/chat/ConversationChatView.tsx:1-225`
- Modify: `frontend/src/components/chat/ConversationChatView.test.tsx`
- Modify: `frontend/src/pages/group/GroupChatPage.tsx`
- Modify: `frontend/src/pages/chat/DirectChatPage.tsx`
- Create: `frontend/src/terminal/useTerminalConversationRegistration.ts`
- Modify: `frontend/src/components/layout/AppSidebar.tsx:1-529`
- Modify: `frontend/src/components/layout/AppLayout.test.tsx`
- Modify: `frontend/src/pages/group/GroupSettingsTab.tsx:1-483`
- Modify: `frontend/src/pages/group/GroupManagementI18n.test.tsx`
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`

**Interfaces:**
- Consumes: `useTerminalConversationRegistration`, `useTerminalRuntime`, and terminal cleanup actions.
- Produces: complete group/direct-chat integration and user-visible localized behavior.

- [ ] **Step 1: Extend failing chat tests for workspace registration and shortcut behavior**

Add `workspaceId` to the render fixture and mock the terminal registration hook. Test:

```tsx
it('registers the conversation workspace and toggles the dock from the header', async () => {
  renderConversation({ workspaceId: 'workspace-1' })
  expect(registerMock).toHaveBeenCalledWith('chat-1', 'workspace-1')
  await userEvent.click(screen.getByRole('button', { name: 'Show terminal' }))
  expect(toggleDockMock).toHaveBeenCalledTimes(1)
})

it('toggles with ctrl-backquote outside editable composition', async () => {
  renderConversation({ workspaceId: 'workspace-1' })
  fireEvent.keyDown(window, { key: '`', ctrlKey: true })
  expect(toggleDockMock).toHaveBeenCalledTimes(1)
  const composer = screen.getByRole('textbox')
  fireEvent.keyDown(composer, { key: '`', ctrlKey: true, isComposing: true })
  expect(toggleDockMock).toHaveBeenCalledTimes(1)
})
```

Add deletion tests asserting `closeConversation(id, true)` runs only after a successful direct-chat/group deletion. Add logout coverage asserting `await closeAll(true)` occurs before auth state and query cache are cleared.

- [ ] **Step 2: Run integration tests and verify they fail**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/components/chat/ConversationChatView.test.tsx src/components/layout/AppLayout.test.tsx src/pages/group/GroupManagementI18n.test.tsx
```

Expected: failures mention missing `workspaceId`, terminal button, and cleanup calls.

- [ ] **Step 3: Pass workspace identity into chat views**

Add `workspaceId: string | null` to `ConversationChatViewProps`. Pass `group.data.workspace_id` from `GroupChatPage` and `item.workspace_id` from `DirectChatPage`. In the registration hook, query `useWorkspaces()` and map the ID to:

```ts
type TerminalConversationTarget =
  | { conversationId: string; availability: 'ready'; cwd: string }
  | { conversationId: string; availability: 'loading' }
  | { conversationId: string; availability: 'desktopRequired' }
  | { conversationId: string; availability: 'workspaceRequired' }
  | { conversationId: string; availability: 'localWorkspaceRequired' }
  | { conversationId: string; availability: 'pathRequired' }
```

Register on mount/change and unregister on unmount without closing runtime sessions.

- [ ] **Step 4: Add header control and scoped keyboard shortcut**

Add a `SquareTerminal` icon button beside the workspace button. Its pressed variant follows `isDockOpen`; its label switches between `terminal.show` and `terminal.hide`. Install one window `keydown` listener per mounted chat view. Accept `Ctrl+\`` on Windows/Linux and `Meta+\`` on macOS; ignore `event.isComposing`, repeated events, and events already prevented. Call `preventDefault()` only when the terminal shortcut is handled.

```tsx
useEffect(() => {
  const onKeyDown = (event: KeyboardEvent) => {
    const modifier = navigator.platform.toLowerCase().includes('mac') ? event.metaKey : event.ctrlKey
    if (event.key !== '`' || !modifier || event.isComposing || event.repeat || event.defaultPrevented) return
    event.preventDefault()
    void toggleDock()
  }
  window.addEventListener('keydown', onKeyDown)
  return () => window.removeEventListener('keydown', onKeyDown)
}, [toggleDock])
```

- [ ] **Step 5: Integrate destructive lifecycle cleanup**

- In direct-chat delete confirmation: await backend delete, then `closeConversation(chat.id, true)`, then navigate.
- In group delete confirmation: await backend delete, then `closeConversation(group.id, true)`, then navigate.
- In logout: make the handler async, await `closeAll(true)`, then call `logout()`, clear React Query, close the menu, and navigate.
- If terminal cleanup fails during delete/logout, log the stable lifecycle error without exposing input/output and continue the already-authorized delete/logout so the Rust exit/Drop safeguard remains responsible for final cleanup.

Use this operation order in the three handlers:

```ts
await deleteDirectChat.mutateAsync()
await closeConversation(chat.id, true).catch(logTerminalCleanupError)

await del.mutateAsync(group.id)
await closeConversation(group.id, true).catch(logTerminalCleanupError)

await closeAll(true).catch(logTerminalCleanupError)
logout()
queryClient.clear()
```

- [ ] **Step 6: Add complete English and Chinese terminal copy**

Add a `chat.terminal` object with keys for show/hide, new, rename, close, restart, collapse, maximize, restore, resize, empty, starting, running, exited, exitCode, retry, fullAccessTitle/body/dismiss, desktopRequired, workspaceRequired, localWorkspaceRequired, pathRequired, spawnError, writeError, and cleanupError. Update the typed Chinese shape with the same keys. Use natural Chinese copy and preserve the current resource typing pattern.

```ts
terminal: {
  show: 'Show terminal', hide: 'Hide terminal', new: 'New terminal', rename: 'Rename terminal',
  close: 'Close terminal', restart: 'Restart terminal', collapse: 'Collapse terminal',
  maximize: 'Maximize terminal', restore: 'Restore terminal', resize: 'Resize terminal height',
  empty: 'No terminal tabs.', starting: 'Starting', running: 'Running', exited: 'Exited',
  exitCode: 'Exit code {{code}}', retry: 'Retry', fullAccessTitle: 'Full local shell access',
  fullAccessBody: 'This terminal starts in the workspace but can access other files and processes allowed by your operating-system account.',
  dismiss: 'I understand', desktopRequired: 'Terminal is available only in the desktop app.',
  workspaceRequired: 'Bind a workspace to use the terminal.',
  localWorkspaceRequired: 'Cloud sandbox terminals are not supported yet.',
  pathRequired: 'The local workspace needs an absolute directory.',
  spawnError: 'Unable to start the terminal: {{message}}',
  writeError: 'Unable to write to the terminal: {{message}}',
  cleanupError: 'Terminal cleanup failed: {{message}}',
}
```

The Chinese object must use the identical keys with `显示终端`, `隐藏终端`, `新建终端`, `重命名终端`, `关闭终端`, `重新启动终端`, `折叠终端`, `最大化终端`, `恢复终端`, and a full-access warning that explicitly says the Shell can leave the workspace.

- [ ] **Step 7: Run integration and full frontend checks**

```powershell
pnpm --filter @ag-swarmer/frontend exec vitest run src/components/chat/ConversationChatView.test.tsx src/components/layout/AppLayout.test.tsx src/pages/group/GroupManagementI18n.test.tsx src/terminal
pnpm --filter @ag-swarmer/frontend type-check
pnpm --filter @ag-swarmer/frontend lint
```

Expected: all targeted tests pass, both locales type-check, and lint is clean.

- [ ] **Step 8: Commit chat and lifecycle integration**

```powershell
git add frontend/src/components/chat/ConversationChatView.tsx frontend/src/components/chat/ConversationChatView.test.tsx frontend/src/pages/group/GroupChatPage.tsx frontend/src/pages/chat/DirectChatPage.tsx frontend/src/components/layout/AppSidebar.tsx frontend/src/pages/group/GroupSettingsTab.tsx frontend/src/pages/group/GroupManagementI18n.test.tsx frontend/src/i18n/resources/en-US.ts frontend/src/i18n/resources/zh-CN.ts
git commit -m "feat(terminal): integrate terminal lifecycle with chats"
```

---

### Task 8: Release-Gate Verification and User Documentation

**Files:**
- Modify: `README.md:16-178`
- Verify: all files changed in Tasks 1–7

**Interfaces:**
- Consumes: complete terminal feature.
- Produces: documented behavior and release evidence.

- [ ] **Step 1: Document the desktop terminal behavior and trust boundary**

Add a concise README section stating:

- the terminal is available only in the Tauri desktop app for local workspaces;
- it is a full host shell, not a workspace sandbox;
- `Ctrl/Cmd + \`` toggles the dock;
- tabs continue across route changes and hide-to-tray;
- real exit ends processes and restart restores metadata only;
- Agent tool execution remains separate.

Add this text under the desktop section:

```markdown
### 本地终端

Tauri 桌面端可在绑定的本地工作区中打开多标签交互终端，使用 `Ctrl/Cmd + \`` 展开或折叠。终端是当前系统用户权限下的完整 Shell，不是工作区沙箱，可以访问工作区以外的文件和进程。切换聊天或隐藏到托盘不会停止会话；真正退出应用会结束终端进程。再次启动只恢复标签名称、顺序和启动目录，不恢复命令、输出或旧进程。用户终端与 Agent 工具调用彼此独立。
```

- [ ] **Step 2: Run all automated frontend gates**

```powershell
pnpm --filter @ag-swarmer/frontend test
pnpm type-check
pnpm lint
pnpm build
```

Expected: Vitest has zero failures; type-check and lint exit 0; Vite production build completes.

- [ ] **Step 3: Run all automated Rust gates**

```powershell
cargo fmt --manifest-path frontend/src-tauri/Cargo.toml -- --check
cargo test --manifest-path frontend/src-tauri/Cargo.toml
cargo clippy --manifest-path frontend/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

Expected: formatting is clean; desktop and backend tests pass; Clippy reports no warnings.

- [ ] **Step 4: Run the ignored Windows PTY integration test**

```powershell
cargo test --manifest-path frontend/src-tauri/Cargo.toml terminal::native::tests::windows_real_pty_smoke -- --ignored --nocapture
```

Expected: PowerShell emits `PTY_OK`, accepts a resize to 100x30, exits, and leaves no child process.

- [ ] **Step 5: Perform the Windows desktop manual acceptance pass**

Run `pnpm desktop:dev`, bind one group and one direct chat to local workspaces, then verify:

1. First open shows the full-host-access notice once.
2. PowerShell renders ANSI color, Chinese text, and emoji correctly.
3. History, Tab completion, Ctrl+C, selection, copy, and paste work.
4. Two tabs retain independent working directories and foreground processes.
5. Dragging, keyboard resizing, collapse, maximize, and restore update PTY dimensions without covering Composer.
6. Switching chats preserves each chat's tabs and running output.
7. Hiding the window to tray preserves sessions; tray Exit removes all Shell and descendant processes.
8. Restart restores names/order/directories but shows new prompts and no old output.
9. Browser mode, cloud workspaces, and missing local paths show unavailable states and make no Tauri call.
10. Direct-chat deletion, group deletion, and logout remove the corresponding runtime sessions and persisted metadata.

- [ ] **Step 6: Inspect the final diff and commit documentation**

```powershell
git diff --check
git status --short
git add README.md
git commit -m "docs: document desktop terminal behavior"
```

Expected: `git diff --check` prints nothing; only intentional user-owned pre-existing files remain untracked; README is committed separately.
