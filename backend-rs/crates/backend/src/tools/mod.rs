//! Workspace-scoped tools for provider-native tool calls.
//!
//! This module hosts the safe filesystem tools an agent may invoke against a
//! configured local workspace, together with the path-safety resolver that
//! keeps every access inside the workspace root.
//!
//! This slice (Task 8a) implements path safety and the file tools `Read`,
//! `Write`, `Edit`, `Glob` and `Grep` via [`workspace::WorkspaceTools`]. The
//! network and shell tools (`Bash`, `Fetch`, `WebSearch`, `AskUser`, the media
//! and planning stubs) and the runtime tool loop arrive in Task 8b; the
//! [`ToolStatus`]/[`ToolResult`]/[`ToolError`] types defined here are shaped so
//! those additions slot in without changing the file-tool API.

pub mod path_safety;
pub mod workspace;

pub use path_safety::resolve_workspace_path;
pub use workspace::{
    WorkspaceTools, MAX_FILE_BYTES, MAX_GLOB_RESULTS, MAX_GREP_RESULTS, MAX_READ_LINES,
    MAX_WRITE_BYTES,
};

use thiserror::Error;

/// Failure returned by a workspace tool.
///
/// [`ToolError::Invalid`] mirrors Python's `WorkspaceToolError`: the request was
/// rejected for a reason the caller (and the model) should see, such as bad
/// arguments, a path that escapes the workspace root, a missing file, or a size
/// limit. [`ToolError::Io`] carries an unexpected underlying filesystem failure.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The request was rejected before or during execution; the message is safe
    /// to surface back to the model.
    #[error("{0}")]
    Invalid(String),
    /// An underlying filesystem operation failed unexpectedly.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl ToolError {
    /// Construct an [`ToolError::Invalid`] from any string-like message.
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        ToolError::Invalid(message.into())
    }
}

/// Coarse status of a completed tool invocation.
///
/// File tools always complete with [`ToolStatus::Completed`]. The remaining
/// variants exist for the Task 8b tools whose Python counterparts return
/// "controlled results" (a structured status instead of executing): for example
/// a tool invoked without a configured local workspace reports
/// [`ToolStatus::WorkspaceRequired`], an unconfigured provider reports
/// [`ToolStatus::SetupRequired`], and `AskUser` reports
/// [`ToolStatus::WaitingForUser`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// The tool ran and produced output.
    Completed,
    /// The tool needs provider configuration that is absent.
    SetupRequired,
    /// The tool needs a local workspace that is not configured.
    WorkspaceRequired,
    /// The tool is pausing for human input.
    WaitingForUser,
}

/// Successful outcome of a tool invocation: a status plus the text the runtime
/// hands back to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Coarse outcome status.
    pub status: ToolStatus,
    /// Human/model-readable output text.
    pub output: String,
}

impl ToolResult {
    /// A [`ToolStatus::Completed`] result wrapping `output`.
    pub fn completed(output: impl Into<String>) -> Self {
        ToolResult {
            status: ToolStatus::Completed,
            output: output.into(),
        }
    }
}
