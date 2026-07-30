//! Workspace-scoped tools for provider-native tool calls.
//!
//! This module hosts the safe filesystem tools an agent may invoke against a
//! configured local workspace, together with the path-safety resolver that
//! keeps every access inside the workspace root.
//!
//! Task 8a implemented path safety and the file tools `Read`, `Write`, `Edit`,
//! `Glob` and `Grep` via [`workspace::WorkspaceTools`]. Task 8b completes the
//! module: the guarded shell tool [`bash`], the bounded HTTP reader [`http`],
//! Tavily-backed search in [`web_search`], the non-executing "controlled" tools
//! in [`controlled`] (`AskUser` and the media/planning stubs), and the [`ToolExecutor`] facade in
//! [`executor`] that dispatches a named tool with JSON arguments and maps every
//! internal failure to model-safe text.

pub mod bash;
pub mod controlled;
pub mod executor;
pub mod http;
pub mod path_safety;
pub(crate) mod web_search;
pub mod workspace;

pub use executor::{McpMount, ToolExecutor};
pub use path_safety::resolve_workspace_path;
pub(crate) use web_search::TavilySearchConfig;
pub use workspace::{
    FileEdit, WorkspaceMount, WorkspaceTools, MAX_FILE_BYTES, MAX_GLOB_RESULTS, MAX_GREP_RESULTS,
    MAX_READ_LINES, MAX_WRITE_BYTES, SELF_MOUNT_NAME,
};

use serde_json::Value;
use thiserror::Error;

/// Skill instructions mounted into a tool executor for runtime inspection.
///
/// This is an in-memory contract only. `SkillManager` may list metadata and
/// return stored markdown instructions, but it must not load files or execute
/// arbitrary skill resources.
#[derive(Debug, Clone, PartialEq)]
pub struct MountedSkill {
    pub name: String,
    pub description: Option<String>,
    pub metadata: Value,
    pub body_markdown: String,
}

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

    /// Render this error as text that is always safe to return to the model.
    ///
    /// [`ToolError::Invalid`] messages are constructed by this crate and never
    /// contain absolute local paths, so they pass through unchanged.
    /// [`ToolError::Io`] wraps a raw OS error whose `Display` can embed the
    /// absolute path that failed; it is collapsed to a generic message so no
    /// local filesystem layout leaks back to the model.
    pub fn model_safe_message(&self) -> String {
        match self {
            ToolError::Invalid(message) => message.clone(),
            ToolError::Io(_) => "the tool failed due to an internal error".to_string(),
        }
    }
}

/// Coarse status of a completed tool invocation.
///
/// File tools always complete with [`ToolStatus::Completed`]. The remaining
/// variants describe the "controlled results" of the Task 8b tools whose Python
/// counterparts return a structured status instead of executing: a tool invoked
/// without a configured local workspace reports [`ToolStatus::WorkspaceRequired`],
/// an unconfigured provider reports [`ToolStatus::SetupRequired`], a required
/// `AskUser` prompt reports [`ToolStatus::WaitingForUser`] (an optional one
/// [`ToolStatus::InputRequested`]), `ExitPlanMode` reports
/// [`ToolStatus::ApprovalRequired`], and any rejected request or unknown tool
/// reports [`ToolStatus::Failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// The tool ran and produced output.
    Completed,
    /// The tool needs provider configuration that is absent.
    SetupRequired,
    /// The tool needs a local workspace that is not configured.
    WorkspaceRequired,
    /// The tool is pausing for required human input.
    WaitingForUser,
    /// The tool requested optional, non-blocking human input.
    InputRequested,
    /// The tool produced a plan that needs user approval before proceeding.
    ApprovalRequired,
    /// The request was rejected (bad arguments, an unknown tool, or a guarded
    /// failure); `output` carries model-safe text explaining why.
    Failed,
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
