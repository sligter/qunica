//! Staged configuration changes.
//!
//! `AppPropose` validates a payload against the same validators the UI path runs,
//! writes a `pending` row to `app_actions`, and returns `APPROVAL_REQUIRED`.
//! Only [`crate::api::app_actions::approve`] applies it; the UI may invoke that
//! endpoint either from an approval card or from the user's auto-approval mode.
//!
//! Validating at propose time rather than at approve time is deliberate: a
//! staged proposal that cannot apply is worse than a refusal, because the user
//! only discovers it after choosing to trust it.

use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use super::{AppControlContext, TargetKind};
use crate::tools::{ToolError, ToolResult, ToolStatus};

/// What a proposal may do to a kind.
///
/// The allowlist is the security boundary, so it is stated once, positively,
/// and consulted before anything else. Absence is refusal:
///
/// - **Deletion** is absent for every kind. An agent that can delete can
///   destroy work no approval dialog fully conveys the scope of.
/// - **Provider** is absent entirely. Its only interesting field is the API
///   key, and a model must never be able to set one.
/// - **MCP** allows only `http`/`sse`. A `stdio` server launches a local
///   process with attacker-chosen argv and environment.
/// - **Settings** is absent: its writable fields are the Tavily key and the
///   workspace root, one secret and one filesystem path.
fn is_allowed(kind: TargetKind, action: Action) -> bool {
    matches!(
        (kind, action),
        (TargetKind::Agent, Action::Create)
            | (TargetKind::Agent, Action::Update)
            | (TargetKind::Skill, Action::Create)
            | (TargetKind::Skill, Action::Update)
            | (TargetKind::Workspace, Action::Create)
            | (TargetKind::Workspace, Action::Update)
            | (TargetKind::Group, Action::Create)
            | (TargetKind::Group, Action::Update)
            | (TargetKind::Mcp, Action::Create)
            | (TargetKind::Mcp, Action::Update)
            | (TargetKind::Chat, Action::Create)
            | (TargetKind::Chat, Action::Update)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Create,
    Update,
}

impl Action {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
        }
    }
}

/// Stage a configuration change for the user to approve.
pub(crate) async fn propose(
    ctx: &AppControlContext,
    args: &Value,
) -> Result<ToolResult, ToolError> {
    let kind = parse_kind(args)?;
    let action = parse_action(args)?;

    if !is_allowed(kind, action) {
        return Err(ToolError::invalid(refusal_reason(kind, action)));
    }

    let target_id = args
        .get("target_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if action == Action::Update && target_id.is_none() {
        return Err(ToolError::invalid("target_id is required to update"));
    }

    let payload = args.get("payload").cloned().unwrap_or_else(|| json!({}));
    if !payload.is_object() {
        return Err(ToolError::invalid("payload must be an object"));
    }

    // Reject the fields this kind must never carry before anything else looks
    // at the payload, so a refused field cannot reach storage even as text.
    reject_forbidden_fields(kind, &payload)?;

    // Dry-run the real validators. Anything that would fail on apply fails
    // here instead, while it is still the model's problem rather than the
    // user's.
    crate::api::app_actions::validate_proposal(ctx, kind, action, target_id, &payload).await?;

    let summary = summarize(ctx, kind, action, target_id, &payload).await?;
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO app_actions \
         (id, owner_id, conversation_id, tool_call_id, target_kind, action, target_id, summary, \
          payload_json, status, created_at) \
         VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?, 'pending', ?)",
    )
    .bind(&id)
    .bind(ctx.owner_id())
    .bind(ctx.conversation_id())
    .bind(kind.as_str())
    .bind(action.as_str())
    .bind(target_id)
    .bind(&summary)
    .bind(payload.to_string())
    .bind(now_rfc3339())
    .execute(ctx.pool())
    .await
    .map_err(|_| ToolError::invalid("could not stage that change"))?;

    Ok(ToolResult {
        status: ToolStatus::ApprovalRequired,
        output: serde_json::to_string(&json!({
            "tool": "AppPropose",
            "status": "APPROVAL_REQUIRED",
            "action_id": id,
            "target_kind": kind.as_str(),
            "action": action.as_str(),
            "summary": summary,
            "message": format!(
                "Staged: {summary}. Nothing has changed yet — the user must approve it. \
                 Tell them what you proposed and that it is waiting for their approval."
            ),
        }))
        .unwrap_or_else(|_| "{}".to_string()),
    })
}

/// Hand the user a prefilled form for a change that cannot be staged.
///
/// This is the escape hatch for the forbidden set. It writes nothing and
/// promises nothing; the user completes the form themselves.
pub(crate) fn prefill(args: &Value) -> Result<ToolResult, ToolError> {
    let kind = parse_kind(args)?;
    let action = parse_action(args)?;
    let fields = args.get("fields").cloned().unwrap_or_else(|| json!({}));
    if !fields.is_object() {
        return Err(ToolError::invalid("fields must be an object"));
    }

    // Secrets must not be echoed even as a suggestion: the value would land in
    // the transcript, and a model-invented API key helps nobody.
    reject_secret_fields(&fields)?;

    let route = match (kind, action) {
        (TargetKind::Provider, Action::Create) => "/providers/new".to_string(),
        (TargetKind::Provider, Action::Update) => {
            format!("/providers/{}", require_target(args)?)
        }
        (TargetKind::Mcp, Action::Create) => "/mcp-servers/new".to_string(),
        (TargetKind::Mcp, Action::Update) => format!("/mcp-servers/{}", require_target(args)?),
        (TargetKind::Agent, Action::Create) => "/agents/new".to_string(),
        (TargetKind::Agent, Action::Update) => format!("/agents/{}", require_target(args)?),
        (TargetKind::Skill, Action::Create) => "/skills/new".to_string(),
        (TargetKind::Skill, Action::Update) => format!("/skills/{}", require_target(args)?),
        (TargetKind::Workspace, Action::Create) => "/workspaces/new".to_string(),
        (TargetKind::Workspace, Action::Update) => {
            format!("/workspaces/{}", require_target(args)?)
        }
        (TargetKind::Group, _) | (TargetKind::Chat, _) => {
            return Err(ToolError::invalid(
                "groups and chats are created from the chat home screen, not a form",
            ))
        }
    };

    Ok(ToolResult {
        status: ToolStatus::Completed,
        output: serde_json::to_string(&json!({
            "tool": "AppPrefill",
            "status": "COMPLETED",
            "route": route,
            "fields": fields,
            "message": "Give the user this link and tell them which fields to fill in. \
                        You cannot complete this one for them.",
        }))
        .unwrap_or_else(|_| "{}".to_string()),
    })
}

fn require_target(args: &Value) -> Result<String, ToolError> {
    args.get("target_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ToolError::invalid("target_id is required to update"))
}

/// Why a `(kind, action)` pair is refused, in terms the model can relay.
fn refusal_reason(kind: TargetKind, action: Action) -> String {
    match (kind, action) {
        (_, Action::Update) | (_, Action::Create) if kind == TargetKind::Provider => {
            "providers hold an API key and cannot be changed this way. Use AppPrefill to give \
             the user a prefilled provider form instead."
                .to_string()
        }
        _ => format!(
            "{} cannot be {}d this way. Deletions and secret-bearing changes are never staged; \
             use AppPrefill to hand the user a form, or ask them to do it directly.",
            kind.as_str(),
            action.as_str()
        ),
    }
}

/// Fields a proposal may never carry, per kind.
fn reject_forbidden_fields(kind: TargetKind, payload: &Value) -> Result<(), ToolError> {
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    if object.contains_key("api_key") || object.contains_key("tavily_api_key") {
        return Err(ToolError::invalid(
            "a proposal must not carry an API key. Use AppPrefill so the user enters it \
             themselves.",
        ));
    }

    if kind == TargetKind::Mcp {
        let transport = object
            .get("transport")
            .and_then(Value::as_str)
            .unwrap_or("");
        if transport == "stdio" || object.contains_key("command") {
            return Err(ToolError::invalid(
                "an stdio MCP server launches a local process and cannot be staged. Use \
                 AppPrefill so the user reviews the command themselves.",
            ));
        }
        if object.contains_key("env") {
            return Err(ToolError::invalid(
                "environment overrides routinely carry credentials and cannot be staged.",
            ));
        }
        if object.contains_key("headers") {
            return Err(ToolError::invalid(
                "MCP headers routinely carry bearer tokens and cannot be staged. Use \
                 AppPrefill so the user enters them themselves.",
            ));
        }
    }

    if kind == TargetKind::Agent && object.contains_key("is_system") {
        return Err(ToolError::invalid("is_system cannot be set"));
    }

    Ok(())
}

fn reject_secret_fields(fields: &Value) -> Result<(), ToolError> {
    let Some(object) = fields.as_object() else {
        return Ok(());
    };
    for key in ["api_key", "tavily_api_key", "headers", "env"] {
        if object.contains_key(key) {
            return Err(ToolError::invalid(format!(
                "do not suggest a value for '{key}'; leave it for the user to enter"
            )));
        }
    }
    Ok(())
}

/// One line describing the change, shown on the approval card.
///
/// The user decides from this sentence, so it names the thing rather than
/// describing the shape of the request.
async fn summarize(
    ctx: &AppControlContext,
    kind: TargetKind,
    action: Action,
    target_id: Option<&str>,
    payload: &Value,
) -> Result<String, ToolError> {
    if kind == TargetKind::Group {
        if let Some(membership) = payload.get("membership") {
            return summarize_group_membership(ctx, target_id, membership).await;
        }
        if let Some(message) = payload.get("message").and_then(Value::as_str) {
            let supplied_name = payload
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string);
            let name = match (action, supplied_name) {
                (_, Some(name)) => Some(name),
                (Action::Update, None) => {
                    let group_id =
                        target_id.ok_or_else(|| ToolError::invalid("target_id is required"))?;
                    sqlx::query_scalar(
                        "SELECT name FROM groups WHERE id = ? AND owner_id = ? \
                         AND status = 'active' AND conversation_kind = 'group'",
                    )
                    .bind(group_id)
                    .bind(ctx.owner_id())
                    .fetch_optional(ctx.pool())
                    .await
                    .map_err(|_| ToolError::invalid("could not inspect the group"))?
                }
                (Action::Create, None) => None,
            }
            .ok_or_else(|| ToolError::invalid("group not found"))?;
            let message = message_preview(message);
            let changes_group = payload
                .as_object()
                .is_some_and(|fields| fields.keys().any(|key| key != "message"));
            return Ok(match (action, changes_group) {
                (Action::Create, _) => {
                    format!("Create group \"{name}\" and send: \"{message}\"")
                }
                (Action::Update, true) => {
                    format!("Update group \"{name}\" and send: \"{message}\"")
                }
                (Action::Update, false) => format!("Send to group \"{name}\": \"{message}\""),
            });
        }
    }

    if kind == TargetKind::Chat {
        let name: Option<String> = match action {
            Action::Create => {
                let agent_id = payload
                    .get("agent_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::invalid("agent_id is required"))?;
                sqlx::query_scalar(
                    "SELECT name FROM agents \
                     WHERE id = ? AND owner_id = ? AND status = 'active' AND is_system = 0",
                )
                .bind(agent_id)
                .bind(ctx.owner_id())
                .fetch_optional(ctx.pool())
                .await
            }
            Action::Update => {
                let chat_id =
                    target_id.ok_or_else(|| ToolError::invalid("target_id is required"))?;
                sqlx::query_scalar(
                    "SELECT a.name FROM groups g JOIN agents a ON a.id = g.direct_agent_id \
                     WHERE g.id = ? AND g.owner_id = ? AND g.status = 'active' \
                       AND g.conversation_kind = 'direct' AND a.status = 'active' \
                       AND a.is_system = 0",
                )
                .bind(chat_id)
                .bind(ctx.owner_id())
                .fetch_optional(ctx.pool())
                .await
            }
        }
        .map_err(|_| ToolError::invalid("could not inspect the private chat recipient"))?;
        let name = name.ok_or_else(|| ToolError::invalid("private chat recipient not found"))?;
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .map(message_preview);
        return Ok(match (action, message) {
            (Action::Create, Some(message)) => {
                format!("Create a private chat with \"{name}\" and send: \"{message}\"")
            }
            (Action::Create, None) => format!("Create a private chat with \"{name}\""),
            (Action::Update, Some(message)) => {
                format!("Send to \"{name}\" in private chat: \"{message}\"")
            }
            (Action::Update, None) => format!("Send a message to \"{name}\" in private chat"),
        });
    }

    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(match (action, name, target_id) {
        (Action::Create, Some(name), _) => format!("Create {} \"{name}\"", kind.as_str()),
        (Action::Create, None, _) => format!("Create a {}", kind.as_str()),
        (Action::Update, Some(name), _) => {
            format!("Rename {} to \"{name}\"", kind.as_str())
        }
        (Action::Update, None, Some(_)) => {
            let fields = payload
                .as_object()
                .map(|object| {
                    object
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            if fields.is_empty() {
                format!("Update {}", kind.as_str())
            } else {
                format!("Update {} ({fields})", kind.as_str())
            }
        }
        (Action::Update, None, None) => format!("Update {}", kind.as_str()),
    })
}

async fn summarize_group_membership(
    ctx: &AppControlContext,
    target_id: Option<&str>,
    membership: &Value,
) -> Result<String, ToolError> {
    let group_id = target_id.ok_or_else(|| ToolError::invalid("target_id is required"))?;
    let group_name: Option<String> = sqlx::query_scalar(
        "SELECT name FROM groups WHERE id = ? AND owner_id = ? \
         AND status = 'active' AND conversation_kind = 'group'",
    )
    .bind(group_id)
    .bind(ctx.owner_id())
    .fetch_optional(ctx.pool())
    .await
    .map_err(|_| ToolError::invalid("could not inspect the group"))?;
    let group_name = group_name.ok_or_else(|| ToolError::invalid("group not found"))?;
    let operation = membership
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("membership operation is required"))?;

    match operation {
        "add_agent" | "remove_agent" => {
            let agent_id = membership
                .get("agent_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::invalid("agent_id is required"))?;
            let agent_name: Option<String> = sqlx::query_scalar(
                "SELECT name FROM agents WHERE id = ? AND owner_id = ? \
                 AND status = 'active' AND is_system = 0",
            )
            .bind(agent_id)
            .bind(ctx.owner_id())
            .fetch_optional(ctx.pool())
            .await
            .map_err(|_| ToolError::invalid("could not inspect the agent"))?;
            let agent_name = agent_name.ok_or_else(|| ToolError::invalid("agent not found"))?;
            let verb = if operation == "add_agent" {
                "Add"
            } else {
                "Remove"
            };
            let direction = if operation == "add_agent" {
                "to"
            } else {
                "from"
            };
            Ok(format!(
                "{verb} agent \"{agent_name}\" {direction} group \"{group_name}\""
            ))
        }
        "add_user" | "remove_user" => {
            let email = membership
                .get("email")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::invalid("email is required"))?;
            let canonical: Option<String> =
                sqlx::query_scalar("SELECT email FROM users WHERE LOWER(email) = LOWER(?) LIMIT 1")
                    .bind(email.trim())
                    .fetch_optional(ctx.pool())
                    .await
                    .map_err(|_| ToolError::invalid("could not inspect the user"))?;
            let canonical = canonical.ok_or_else(|| ToolError::invalid("user not found"))?;
            let verb = if operation == "add_user" {
                "Add"
            } else {
                "Remove"
            };
            let direction = if operation == "add_user" {
                "to"
            } else {
                "from"
            };
            Ok(format!(
                "{verb} user \"{canonical}\" {direction} group \"{group_name}\""
            ))
        }
        _ => Err(ToolError::invalid("unknown membership operation")),
    }
}

fn message_preview(message: &str) -> String {
    const LIMIT: usize = 80;
    let mut chars = message.trim().chars();
    let preview = chars.by_ref().take(LIMIT).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn parse_kind(args: &Value) -> Result<TargetKind, ToolError> {
    let raw = args
        .get("target_kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("target_kind is required"))?;
    TargetKind::parse(raw).ok_or_else(|| ToolError::invalid(format!("unknown target_kind '{raw}'")))
}

fn parse_action(args: &Value) -> Result<Action, ToolError> {
    let raw = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("action is required"))?;
    Action::parse(raw).ok_or_else(|| {
        ToolError::invalid(format!(
            "unknown action '{raw}'; only create and update can be proposed"
        ))
    })
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
