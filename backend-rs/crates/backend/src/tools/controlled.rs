//! Controlled (non-executing) tools and the shared controlled-result encoder.
//!
//! These are tools that return a structured status instead of performing an action:
//! unconfigured providers, `AskUser`, `TodoWrite`, and `ExitPlanMode`. Each returns
//! a [`ToolResult`] whose `output` is a JSON object carrying the stable `tool` /
//! `status` / `message` shape used by the Rust runtime.

use serde_json::{Map, Value};

use super::{MountedSkill, ToolResult, ToolStatus};

/// Maximum number of choices echoed back by `AskUser`.
const MAX_ASK_CHOICES: usize = 8;
/// Maximum number of todos echoed back by `TodoWrite`.
const MAX_TODOS: usize = 20;

const SKILL_LIST_MESSAGE: &str =
    "Skill list includes metadata only; inspect or activate a skill to load instructions.";
const SKILL_ACTIVATION_MESSAGE: &str =
    "Skill runtime activation records intent only; no arbitrary code was loaded.";

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

/// Controlled result for a tool whose supporting configuration is absent.
///
/// Distinct from a failure: nothing about the request was wrong, the runtime
/// just cannot serve it here. The model should say so rather than retry.
pub fn setup_required(tool: &str, reason: &str) -> ToolResult {
    controlled_result(
        tool,
        "SETUP_REQUIRED",
        ToolStatus::SetupRequired,
        Some(reason),
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

    // The runtime lifts `input_request` onto the `waiting_for_user` stream
    // event, and the UI renders the form from it. Without this key the client
    // falls back to a generic "The agent requested input." and the question is
    // lost, so it travels structured rather than only inside `message`.
    let mut request = Map::new();
    request.insert(
        "question".to_string(),
        Value::String(truncate_chars(question, 1000)),
    );
    request.insert("required".to_string(), Value::Bool(required));
    if !normalized.is_empty() {
        request.insert(
            "input_type".to_string(),
            Value::String("choice".to_string()),
        );
        request.insert("choices".to_string(), Value::Array(normalized.clone()));
    }

    let mut extra = vec![("input_request", Value::Object(request))];
    // `choices` is kept alongside for older clients that read it flat.
    if !normalized.is_empty() {
        extra.push(("choices", Value::Array(normalized)));
    }
    controlled_result("AskUser", status_label, status, Some(&message), extra)
}

pub(crate) fn web_search_setup_required() -> ToolResult {
    controlled_result(
        "WebSearch",
        "SETUP_REQUIRED",
        ToolStatus::SetupRequired,
        Some("No search provider is configured for this agent."),
        Vec::new(),
    )
}

/// `SkillManager`: inspect mounted skill metadata/instructions without loading
/// files or executing arbitrary skill resources.
pub fn skill_manager(
    mounted_skills: &[MountedSkill],
    action: &str,
    skill_name: Option<&str>,
) -> ToolResult {
    match action {
        "list" => skill_manager_list(mounted_skills),
        "inspect" | "activate" => match skill_name.filter(|name| !name.is_empty()) {
            Some(name) => skill_manager_inspect(mounted_skills, name),
            None => skill_manager_list(mounted_skills),
        },
        other => {
            let message = format!("Unsupported skill action: {other}");
            controlled_result(
                "SkillManager",
                "FAILED",
                ToolStatus::Failed,
                Some(&message),
                Vec::new(),
            )
        }
    }
}

fn skill_manager_list(mounted_skills: &[MountedSkill]) -> ToolResult {
    let skills = mounted_skills
        .iter()
        .map(skill_metadata)
        .collect::<Vec<Value>>();
    controlled_result(
        "SkillManager",
        "COMPLETED",
        ToolStatus::Completed,
        Some(SKILL_LIST_MESSAGE),
        vec![("skills", Value::Array(skills))],
    )
}

fn skill_manager_inspect(mounted_skills: &[MountedSkill], skill_name: &str) -> ToolResult {
    let matched = mounted_skills
        .iter()
        .filter(|skill| skill.name == skill_name)
        .map(skill_with_instructions)
        .collect::<Vec<Value>>();
    let (status_label, status) = if matched.is_empty() {
        ("NOT_FOUND", ToolStatus::Failed)
    } else {
        ("COMPLETED", ToolStatus::Completed)
    };
    controlled_result(
        "SkillManager",
        status_label,
        status,
        Some(SKILL_ACTIVATION_MESSAGE),
        vec![("skills", Value::Array(matched))],
    )
}

fn skill_metadata(skill: &MountedSkill) -> Value {
    let mut object = skill_base_object(skill);
    object.insert("metadata".to_string(), skill.metadata.clone());
    Value::Object(object)
}

fn skill_with_instructions(skill: &MountedSkill) -> Value {
    let mut object = skill_base_object(skill);
    object.insert("metadata".to_string(), skill.metadata.clone());
    object.insert(
        "instructions".to_string(),
        Value::String(skill.body_markdown.clone()),
    );
    Value::Object(object)
}

fn skill_base_object(skill: &MountedSkill) -> Map<String, Value> {
    let mut object = Map::new();
    object.insert("name".to_string(), Value::String(skill.name.clone()));
    object.insert(
        "description".to_string(),
        skill
            .description
            .as_ref()
            .map_or(Value::Null, |description| {
                Value::String(description.clone())
            }),
    );
    object
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
