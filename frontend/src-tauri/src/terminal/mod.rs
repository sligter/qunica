use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

pub mod manager;
pub mod native;
pub(crate) mod process_tree;
pub mod protocol;
pub mod shell;

use manager::{EventSink, TerminalManager};
use protocol::{CreateTerminalRequest, TerminalCommandError, TerminalDescriptor, TerminalEvent};

pub struct TauriChannelSink(Channel<TerminalEvent>);

impl EventSink for TauriChannelSink {
    fn send(&self, event: TerminalEvent) -> Result<(), TerminalCommandError> {
        self.0.send(event).map_err(|_| {
            TerminalCommandError::new(
                "terminal.channel_disconnected",
                "Terminal event channel is disconnected",
            )
        })
    }
}

#[tauri::command]
pub fn terminal_create(
    request: CreateTerminalRequest,
    on_event: Channel<TerminalEvent>,
    manager: State<'_, TerminalManager>,
) -> Result<TerminalDescriptor, TerminalCommandError> {
    manager.create(request, Arc::new(TauriChannelSink(on_event)))
}

#[tauri::command]
pub fn terminal_write(
    conversation_id: String,
    session_id: String,
    data: String,
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError> {
    manager.write(&conversation_id, &session_id, data.as_bytes())
}

#[tauri::command]
pub fn terminal_resize(
    conversation_id: String,
    session_id: String,
    cols: u16,
    rows: u16,
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError> {
    manager.resize(&conversation_id, &session_id, cols, rows)
}

#[tauri::command]
pub async fn terminal_close(
    conversation_id: String,
    session_id: String,
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.close(&conversation_id, &session_id))
        .await
        .map_err(|_| terminal_cleanup_join_error())?
}

#[tauri::command]
pub async fn terminal_close_all(
    manager: State<'_, TerminalManager>,
) -> Result<(), TerminalCommandError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.close_all())
        .await
        .map_err(|_| terminal_cleanup_join_error())?
}

fn terminal_cleanup_join_error() -> TerminalCommandError {
    TerminalCommandError::new(
        "terminal.cleanup_join_failed",
        "Terminal cleanup task did not complete",
    )
}
