//! Controlled (non-executing) tools and the shared controlled-result encoder.
//!
//! These mirror the Python oracle's `_controlled_tool_result` helper and the
//! tools that return a structured status instead of performing an action:
//! `WebSearch` (setup-required without a provider), `AskUser`, the media stubs
//! `GenerateImage`/`GenerateVideo`, `TodoWrite`, and `ExitPlanMode`. Each returns
//! a [`ToolResult`] whose `output` is a JSON object carrying the same `tool` /
//! `status` / `message` shape as the Python implementation.

use serde_json::{Map, Value};

use super::{ToolError, ToolResult, ToolStatus};

/// Default number of search results requested.
pub const DEFAULT_SEARCH_RESULTS: u32 = 5;
/// Largest number of search results a caller may request.
pub const MAX_SEARCH_RESULTS: u32 = 20;
/// Largest search query length (in characters).
pub const MAX_SEARCH_QUERY_CHARS: usize = 500;

/// Maximum number of choices echoed back by `AskUser`.
const MAX_ASK_CHOICES: usize = 8;
/// Maximum number of todos echoed back by `TodoWrite`.
const MAX_TODOS: usize = 20;

/// Build a controlled-result `ToolResult` whose output is a JSON object with the
/// given `tool`, `status`, optional `message`, and any extra fields.
fn controlled_result(
    tool: &str,
    status_label: &str,
    status: ToolStatus,
    message: Option<&str>,
    extra: Vec<(&str, Value)>,
) -> ToolResult {
    let mut object = Map::new();
    object.insert("tool".to_string(), Value::String(tool.to_string()));
    object.insert(
        "status".to_string(),
        Value::String(status_label.to_string()),
    );
    if let Some(message) = message {
        object.insert("message".to_string(), Value::String(message.to_string()));
    }
    for (key, value) in extra {
        object.insert(key.to_string(), value);
    }
    let output = serde_json::to_string(&Value::Object(object)).unwrap_or_else(|_| "{}".to_string());
    ToolResult { status, output }
}

/// Truncate `value` to at most `max` characters (not bytes).
fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Controlled result returned when a workspace-scoped tool is invoked without a
/// configured local workspace.
pub fn workspace_required(tool: &str) -> ToolResult {
    controlled_result(
        tool,
        "WORKSPACE_REQUIRED",
        ToolStatus::WorkspaceRequired,
        Some("No local workspace is configured for this agent."),
        Vec::new(),
    )
}

/// `AskUser`: request bounded human input without blocking execution. A required
/// prompt reports `WAITING_FOR_USER`; an optional one reports `INPUT_REQUESTED`.
pub fn ask_user(question: &str, required: bool, choices: &[String]) -> ToolResult {
    let (status_label, status) = if required {
        ("WAITING_FOR_USER", ToolStatus::WaitingForUser)
    } else {
        ("INPUT_REQUESTED", ToolStatus::InputRequested)
    };
    let normalized: Vec<Value> = choices
        .iter()
        .map(|choice| choice.trim())
        .filter(|choice| !choice.is_empty())
        .take(MAX_ASK_CHOICES)
        .map(|choice| Value::String(choice.to_string()))
        .collect();
    let message = format!("Human input requested: {}", truncate_chars(question, 1000));
    let extra = if normalized.is_empty() {
        Vec::new()
    } else {
        vec![("choices", Value::Array(normalized))]
    };
    controlled_result("AskUser", status_label, status, Some(&message), extra)
}

/// `WebSearch`: validate arguments, then report `SETUP_REQUIRED` because no
/// search provider is configured in this runtime.
pub fn web_search(query: &str, max_results: u32) -> Result<ToolResult, ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::invalid("query must be non-empty"));
    }
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(ToolError::invalid(format!(
            "query must be at most {MAX_SEARCH_QUERY_CHARS} characters"
        )));
    }
    if !(1..=MAX_SEARCH_RESULTS).contains(&max_results) {
        return Err(ToolError::invalid(format!(
            "max_results must be between 1 and {MAX_SEARCH_RESULTS}"
        )));
    }
    Ok(controlled_result(
        "WebSearch",
        "SETUP_REQUIRED",
        ToolStatus::SetupRequired,
        Some("No search provider is configured for this agent."),
        Vec::new(),
    ))
}

/// `GenerateImage`: media stub that reports `SETUP_REQUIRED` without calling any
/// external provider.
pub fn generate_image(prompt: &str) -> ToolResult {
    let message = format!(
        "Image generation provider is not configured. Requested prompt: {}",
        truncate_chars(prompt, 1000)
    );
    controlled_result(
        "GenerateImage",
        "SETUP_REQUIRED",
        ToolStatus::SetupRequired,
        Some(&message),
        Vec::new(),
    )
}

/// `GenerateVideo`: media stub that reports `SETUP_REQUIRED` without calling any
/// external provider.
pub fn generate_video(prompt: &str) -> ToolResult {
    let message = format!(
        "Video generation provider is not configured. Requested prompt: {}",
        truncate_chars(prompt, 1000)
    );
    controlled_result(
        "GenerateVideo",
        "SETUP_REQUIRED",
        ToolStatus::SetupRequired,
        Some(&message),
        Vec::new(),
    )
}

/// `TodoWrite`: echo a bounded list of todos back as a completed result.
pub fn todo_write(todos: Vec<String>) -> ToolResult {
    let bounded: Vec<Value> = todos
        .into_iter()
        .take(MAX_TODOS)
        .map(Value::String)
        .collect();
    controlled_result(
        "TodoWrite",
        "COMPLETED",
        ToolStatus::Completed,
        None,
        vec![("todos", Value::Array(bounded))],
    )
}

/// `ExitPlanMode`: report a plan that needs user approval; performs no action.
pub fn exit_plan_mode(plan: &str) -> ToolResult {
    let message = format!(
        "Plan ready for user approval: {}",
        truncate_chars(plan, 2000)
    );
    controlled_result(
        "ExitPlanMode",
        "APPROVAL_REQUIRED",
        ToolStatus::ApprovalRequired,
        Some(&message),
        Vec::new(),
    )
}
