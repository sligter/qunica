//! Agent-as-tool resolution for group runtime handoffs.
//!
//! The provider-facing tool accepts an assistant selector plus a task. The
//! resolver below keeps the execution boundary narrow: only assistants
//! explicitly enabled on the caller and active in the same group can be
//! dispatched.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;

pub const AGENT_AS_TOOL_NAME: &str = "AgentAsTool";

#[derive(Debug, Clone)]
pub(crate) struct AgentAsToolCall {
    pub tool_call_id: String,
    pub requested_agent: String,
    pub task: String,
    pub instructions: Option<String>,
    pub mode: AgentAsToolMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentAsToolMode {
    Call,
    Handoff,
}

impl AgentAsToolCall {
    pub fn from_args(tool_call_id: String, args: &Value) -> Result<Self, AgentAsToolFailure> {
        let requested_agent = first_string(
            args,
            &[
                "agent_id",
                "assistant_agent_id",
                "assistant_id",
                "assistant",
                "name",
                "display_name",
            ],
        )
        .ok_or_else(|| AgentAsToolFailure::failed("assistant selector is required"))?;
        let task = first_string(args, &["task"])
            .ok_or_else(|| AgentAsToolFailure::failed("task is required"))?;
        let instructions = first_string(args, &["instructions"])
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        let mode = args
            .get("mode")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| AgentAsToolFailure::failed("mode must be call or handoff"))?
            .unwrap_or(AgentAsToolMode::Handoff);

        // Models name an assistant the way the roster prints it, which is with
        // the `@` they see in every group message. Stripping it here means a
        // call written the obvious way resolves instead of failing on a
        // character the selector never contained.
        let requested_agent = requested_agent
            .trim()
            .trim_start_matches('@')
            .trim()
            .to_string();
        if requested_agent.is_empty() {
            return Err(AgentAsToolFailure::failed(
                "assistant selector must not be empty",
            ));
        }
        let task = task.trim().to_string();
        if task.is_empty() {
            return Err(AgentAsToolFailure::failed("task must not be empty"));
        }

        Ok(Self {
            tool_call_id,
            requested_agent,
            task,
            instructions,
            mode,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CallerAgent {
    pub agent_id: String,
    pub owner_id: String,
    pub display_name: String,
    pub tool_config_json: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AssistantMember {
    pub agent_id: String,
    pub name: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentAsToolDispatch {
    pub helper: AssistantMember,
    pub content: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentAsToolFailure {
    pub status: &'static str,
    pub message: String,
}

impl AgentAsToolFailure {
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            status: "failed",
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: "unavailable",
            message: message.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ToolConfig {
    #[serde(default)]
    assistant_agents: Vec<AssistantSelection>,
}

#[derive(Debug, Deserialize)]
struct AssistantSelection {
    agent_id: String,
    #[serde(default = "enabled_default")]
    enabled: bool,
}

fn enabled_default() -> bool {
    true
}

pub(crate) async fn resolve_dispatch(
    pool: &SqlitePool,
    group_id: &str,
    caller: &CallerAgent,
    call: &AgentAsToolCall,
    muted_agent_ids: &HashSet<String>,
) -> Result<AgentAsToolDispatch, AgentAsToolFailure> {
    let bound_ids = enabled_assistant_ids(caller)?;
    if bound_ids.is_empty() {
        return Err(AgentAsToolFailure::unavailable(
            "assistant is not enabled for this agent",
        ));
    }

    let requested = call.requested_agent.trim();
    let requested_folded = requested.to_lowercase();
    let mut matched_bound_outside_group: Option<String> = None;

    let rows = load_bound_group_members(
        pool,
        group_id,
        &caller.owner_id,
        &bound_ids,
        muted_agent_ids,
    )
    .await
    .map_err(|_| AgentAsToolFailure::failed("failed to resolve assistant agent"))?;

    for helper in rows {
        if helper.agent_id == caller.agent_id {
            if matches_requested(&helper, requested, &requested_folded) {
                return Err(AgentAsToolFailure::unavailable(
                    "agent cannot delegate to itself",
                ));
            }
            continue;
        }
        if matches_requested(&helper, requested, &requested_folded) {
            let content = build_dispatch_content(
                &helper.display_name,
                &caller.display_name,
                &call.task,
                call.instructions.as_deref(),
            )?;
            return Ok(AgentAsToolDispatch { helper, content });
        }
    }

    for bound_id in &bound_ids {
        if bound_id.eq_ignore_ascii_case(requested) {
            matched_bound_outside_group = Some(bound_id.clone());
            break;
        }
    }
    if matched_bound_outside_group.is_none() {
        matched_bound_outside_group =
            load_bound_agent_name_match(pool, &caller.owner_id, &bound_ids, &requested_folded)
                .await
                .map_err(|_| AgentAsToolFailure::failed("failed to resolve assistant agent"))?;
    }

    if matched_bound_outside_group.is_some() {
        return Err(AgentAsToolFailure::unavailable(
            "assistant agent must be added to this group before AgentAsTool can dispatch to it",
        ));
    }

    Err(AgentAsToolFailure::unavailable(
        "assistant is not enabled for this agent",
    ))
}

/// The assistants a caller could actually dispatch to on this turn.
///
/// `bound` counts what the owner selected on the agent; `dispatchable` holds
/// only the ones that survive every gate the resolver applies later — same
/// owner, active, an active member of *this* group, not muted, not the caller.
/// The two are reported separately because the gap between them is the whole
/// diagnosis when delegation quietly does nothing: assistants were bound, but
/// none of them are in the room.
pub(crate) struct AssistantRoster {
    pub bound: usize,
    pub dispatchable: Vec<AssistantMember>,
}

/// Resolve the roster used to describe `AgentAsTool` to the model.
///
/// Never fails the turn: a caller whose tool configuration will not parse, or
/// whose lookup errors, simply has nothing to dispatch to, which is how the
/// tool then presents itself. The dispatch path re-runs the same checks and is
/// the authority on any individual call.
pub(crate) async fn dispatchable_assistants(
    pool: &SqlitePool,
    group_id: &str,
    caller: &CallerAgent,
    muted_agent_ids: &HashSet<String>,
) -> AssistantRoster {
    let bound_ids = enabled_assistant_ids(caller).unwrap_or_default();
    let mut dispatchable = load_bound_group_members(
        pool,
        group_id,
        &caller.owner_id,
        &bound_ids,
        muted_agent_ids,
    )
    .await
    .unwrap_or_default();
    dispatchable.retain(|helper| helper.agent_id != caller.agent_id);
    AssistantRoster {
        bound: bound_ids.len(),
        dispatchable,
    }
}

fn first_string<'a>(args: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| args.get(*name)?.as_str())
}

fn enabled_assistant_ids(caller: &CallerAgent) -> Result<Vec<String>, AgentAsToolFailure> {
    let Some(raw) = caller.tool_config_json.as_deref() else {
        return Ok(Vec::new());
    };
    let config: ToolConfig = serde_json::from_str(raw)
        .map_err(|_| AgentAsToolFailure::failed("invalid assistant tool configuration"))?;
    Ok(config
        .assistant_agents
        .into_iter()
        .filter(|selection| selection.enabled)
        .map(|selection| selection.agent_id)
        .collect())
}

fn matches_requested(helper: &AssistantMember, requested: &str, requested_folded: &str) -> bool {
    helper.agent_id.eq_ignore_ascii_case(requested)
        || helper.name.to_lowercase() == requested_folded
        || helper.display_name.to_lowercase() == requested_folded
}

fn build_dispatch_content(
    helper_display: &str,
    caller_display: &str,
    task: &str,
    instructions: Option<&str>,
) -> Result<String, AgentAsToolFailure> {
    let task = task.trim();
    if task.is_empty() {
        return Err(AgentAsToolFailure::failed("task must not be empty"));
    }
    let mut content = format!("@{helper_display} {task}");
    if let Some(instructions) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        content.push_str("\n\n");
        content.push_str(&format!(
            "Instructions from @{caller_display}: {instructions}"
        ));
    }
    Ok(content)
}

async fn load_bound_group_members(
    pool: &SqlitePool,
    group_id: &str,
    owner_id: &str,
    bound_ids: &[String],
    muted_agent_ids: &HashSet<String>,
) -> anyhow::Result<Vec<AssistantMember>> {
    let mut helpers = Vec::new();
    for bound_id in bound_ids {
        if muted_agent_ids.contains(bound_id) {
            continue;
        }
        let row = sqlx::query_as::<_, AssistantRow>(
            "SELECT a.id, a.name, ga.display_name \
             FROM group_agents ga \
             JOIN agents a ON a.id = ga.agent_id \
             WHERE ga.group_id = ? AND ga.agent_id = ? AND ga.status = 'active' \
               AND a.status = 'active' AND a.owner_id = ?",
        )
        .bind(group_id)
        .bind(bound_id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await?;
        if let Some(row) = row {
            helpers.push(row.into());
        }
    }
    Ok(helpers)
}

async fn load_bound_agent_name_match(
    pool: &SqlitePool,
    owner_id: &str,
    bound_ids: &[String],
    requested_folded: &str,
) -> anyhow::Result<Option<String>> {
    for bound_id in bound_ids {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM agents WHERE id = ? AND owner_id = ? AND status = 'active'",
        )
        .bind(bound_id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await?;
        if let Some((id, name)) = row {
            if id.eq_ignore_ascii_case(requested_folded) || name.to_lowercase() == requested_folded
            {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

#[derive(sqlx::FromRow)]
struct AssistantRow {
    id: String,
    name: String,
    display_name: Option<String>,
}

impl From<AssistantRow> for AssistantMember {
    fn from(row: AssistantRow) -> Self {
        let display_name = row.display_name.unwrap_or_else(|| row.name.clone());
        Self {
            agent_id: row.id,
            name: row.name,
            display_name,
        }
    }
}
