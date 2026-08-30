//! Agent-as-tool resolution for group runtime handoffs.
//!
//! The provider-facing tool accepts an assistant selector plus a task. The
//! resolver below keeps the execution boundary narrow: only assistants
//! explicitly enabled on the caller and active in the same group can be
//! dispatched.
//!
//! `fan_out` names several assistants in one call, each with its own task. It
//! is the same private dispatch `call` performs, repeated: the targets run one
//! after another — the group runtime dispatches agents sequentially — and every
//! result comes back to the caller together. What it removes is the round trip:
//! delegating to three helpers used to cost three provider requests carrying the
//! caller's whole context, and now costs one.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;

pub const AGENT_AS_TOOL_NAME: &str = "AgentAsTool";

/// The most assistants one `fan_out` call may name.
///
/// The scheduler's step budget is the real ceiling — each target spends one
/// agent step, and a fan-out that exhausts it leaves the caller holding partial
/// results — and the schema already narrows the list to the helpers this caller
/// can reach. This only stops a model that pastes the roster in twice.
pub(crate) const MAX_FAN_OUT_TARGETS: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct AgentAsToolCall {
    pub tool_call_id: String,
    pub mode: AgentAsToolMode,
    /// Exactly one entry for `call` and `handoff`; two or more for `fan_out`.
    pub targets: Vec<AgentAsToolTarget>,
}

/// One assistant and the work meant for it.
#[derive(Debug, Clone)]
pub(crate) struct AgentAsToolTarget {
    pub requested_agent: String,
    pub task: String,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentAsToolMode {
    Call,
    Handoff,
    FanOut,
}

impl AgentAsToolCall {
    pub fn from_args(tool_call_id: String, args: &Value) -> Result<Self, AgentAsToolFailure> {
        // Mode is read first because it decides where the targets live. A
        // `fan_out` carries them in `dispatches` and leaves the top-level
        // selector empty, so complaining about a missing `assistant` before the
        // mode is known would name the wrong field.
        let mode: AgentAsToolMode =
            serde_json::from_value(args.get("mode").cloned().ok_or_else(|| {
                AgentAsToolFailure::failed("mode is required: use call, handoff, or fan_out")
            })?)
            .map_err(|_| AgentAsToolFailure::failed("mode must be call, handoff, or fan_out"))?;

        let targets = match mode {
            AgentAsToolMode::Call | AgentAsToolMode::Handoff => vec![parse_target(args)?],
            AgentAsToolMode::FanOut => parse_fan_out_targets(args)?,
        };

        Ok(Self {
            tool_call_id,
            mode,
            targets,
        })
    }
}

/// Read one `{assistant, task, instructions}` object.
///
/// Single-target calls carry these at the top level and `fan_out` entries carry
/// the same three fields, so both parse through here.
fn parse_target(value: &Value) -> Result<AgentAsToolTarget, AgentAsToolFailure> {
    let requested_agent = first_string(
        value,
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
    let task = first_string(value, &["task"])
        .ok_or_else(|| AgentAsToolFailure::failed("task is required"))?;
    let instructions = first_string(value, &["instructions"])
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);

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

    Ok(AgentAsToolTarget {
        requested_agent,
        task,
        instructions,
    })
}

/// Read the `dispatches` list of a `fan_out` call.
///
/// A single entry is rejected rather than quietly demoted to `call`: the two
/// modes report differently, and a model that meant to fan out should learn it
/// named one target. Every rejection here happens before anything is
/// dispatched, so the whole call stays retryable rather than half-executed.
fn parse_fan_out_targets(args: &Value) -> Result<Vec<AgentAsToolTarget>, AgentAsToolFailure> {
    let entries = args
        .get("dispatches")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AgentAsToolFailure::failed(
                "fan_out requires dispatches: a list of {assistant, task} entries",
            )
        })?;
    if entries.len() < 2 {
        return Err(AgentAsToolFailure::failed(
            "fan_out requires at least two dispatches; use call for a single assistant",
        ));
    }
    if entries.len() > MAX_FAN_OUT_TARGETS {
        return Err(AgentAsToolFailure::failed(format!(
            "fan_out accepts at most {MAX_FAN_OUT_TARGETS} dispatches"
        )));
    }

    let mut targets = Vec::with_capacity(entries.len());
    let mut seen = HashSet::new();
    for entry in entries {
        let target = parse_target(entry)?;
        // Each helper runs at most once per turn, so a repeated name would
        // dispatch once and report `already_scheduled` for the copy. Saying it
        // up front costs the caller nothing and half-running the call would.
        if !seen.insert(target.requested_agent.to_lowercase()) {
            return Err(AgentAsToolFailure::failed(format!(
                "fan_out must name each assistant once, but '{}' appears twice",
                target.requested_agent
            )));
        }
        targets.push(target);
    }
    Ok(targets)
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

    pub fn already_scheduled(message: impl Into<String>) -> Self {
        Self {
            status: "already_scheduled",
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
    target: &AgentAsToolTarget,
    muted_agent_ids: &HashSet<String>,
) -> Result<AgentAsToolDispatch, AgentAsToolFailure> {
    let bound_ids = enabled_assistant_ids(caller)?;
    if bound_ids.is_empty() {
        return Err(AgentAsToolFailure::unavailable(
            "assistant is not enabled for this agent",
        ));
    }

    let requested = target.requested_agent.trim();
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
                &target.task,
                target.instructions.as_deref(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(args: Value) -> Result<AgentAsToolCall, AgentAsToolFailure> {
        AgentAsToolCall::from_args("call_1".to_owned(), &args)
    }

    #[test]
    fn single_target_modes_read_the_top_level_fields() {
        let call =
            parse(json!({"assistant": "@Helper", "task": " audit ", "mode": "call"})).unwrap();

        assert_eq!(call.mode, AgentAsToolMode::Call);
        assert_eq!(call.targets.len(), 1);
        // The `@` is roster punctuation, not part of any identifier.
        assert_eq!(call.targets[0].requested_agent, "Helper");
        assert_eq!(call.targets[0].task, "audit");
        assert_eq!(call.targets[0].instructions, None);
    }

    #[test]
    fn fan_out_gives_every_assistant_its_own_task() {
        let call = parse(json!({
            "mode": "fan_out",
            "dispatches": [
                {"assistant": "Alice", "task": "review module a"},
                {"assistant": "@Bob", "task": "review module b", "instructions": "cite lines"},
            ],
        }))
        .unwrap();

        assert_eq!(call.mode, AgentAsToolMode::FanOut);
        assert_eq!(call.targets.len(), 2);
        assert_eq!(call.targets[0].task, "review module a");
        assert_eq!(call.targets[1].requested_agent, "Bob");
        assert_eq!(call.targets[1].instructions.as_deref(), Some("cite lines"));
    }

    /// The mode decides which fields are read, so it is reported before any of
    /// them. A `fan_out` has no top-level `assistant`, and telling the model
    /// that field is missing would send it to fix the wrong thing.
    #[test]
    fn mode_is_reported_before_the_fields_it_selects() {
        let missing = parse(json!({"assistant": "Helper", "task": "audit"})).unwrap_err();
        assert!(missing.message.contains("mode is required"));

        let unknown =
            parse(json!({"assistant": "Helper", "task": "audit", "mode": "delegate"})).unwrap_err();
        assert!(unknown.message.contains("call, handoff, or fan_out"));

        let no_dispatches = parse(json!({"mode": "fan_out"})).unwrap_err();
        assert!(no_dispatches
            .message
            .contains("fan_out requires dispatches"));
    }

    /// Every fan-out rejection has to land before the first dispatch runs, or
    /// the caller is left with half a fan-out it cannot retry: the helpers that
    /// did run are spent for the turn.
    #[test]
    fn fan_out_rejects_a_list_it_cannot_run_whole() {
        let single = parse(json!({
            "mode": "fan_out",
            "dispatches": [{"assistant": "Alice", "task": "review"}],
        }))
        .unwrap_err();
        assert!(single.message.contains("at least two dispatches"));

        let duplicated = parse(json!({
            "mode": "fan_out",
            "dispatches": [
                {"assistant": "Alice", "task": "review a"},
                {"assistant": "alice", "task": "review b"},
            ],
        }))
        .unwrap_err();
        assert!(duplicated.message.contains("appears twice"));

        let oversized: Vec<Value> = (0..=MAX_FAN_OUT_TARGETS)
            .map(|index| json!({"assistant": format!("Helper{index}"), "task": "review"}))
            .collect();
        let too_many = parse(json!({"mode": "fan_out", "dispatches": oversized})).unwrap_err();
        assert!(too_many.message.contains("at most"));

        let incomplete = parse(json!({
            "mode": "fan_out",
            "dispatches": [
                {"assistant": "Alice", "task": "review a"},
                {"assistant": "Bob"},
            ],
        }))
        .unwrap_err();
        assert!(incomplete.message.contains("task is required"));
    }
}
