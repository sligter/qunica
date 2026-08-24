//! Identity-aware group conversation loading and provider prompt rendering.
//!
//! Message content from humans and peer agents is untrusted. Renderers preserve
//! the speaker identity in an escaped host-controlled envelope; only the
//! current agent's own history is represented as assistant output.
//!
//! Attachment paths are relative to the *conversation* workspace, so a renderer
//! must be told whether the reading agent can actually address that root — see
//! [`AttachmentAccess`]. Handing an isolated agent a bare relative path would
//! silently resolve it under a different root.

use crate::llm::{ChatMessage, ToolCall};
use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConversationAttachment {
    pub id: String,
    pub path: String,
    pub name: String,
    pub mime_type: String,
    pub size: i64,
}

/// Whether the agent being prompted can read the conversation workspace that
/// attachment paths are relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentAccess {
    /// The conversation workspace is addressable; render the relative path.
    Readable,
    /// The agent runs somewhere else entirely; render the attachment as
    /// metadata only, with no path it could resolve against the wrong root.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationActor {
    Human { id: String, display_name: String },
    Agent { id: String, display_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub id: String,
    pub actor: ConversationActor,
    pub content: String,
    pub turn_id: Option<String>,
    pub dispatch_id: Option<String>,
    pub reply_to_message_id: Option<String>,
    pub attachments: Vec<ConversationAttachment>,
    pub tool_calls: Vec<ConversationToolCall>,
    /// Thinking blocks this agent turn produced, in order.
    ///
    /// Replayed only into the agent's own history, and only reaches a provider
    /// that asked for its reasoning back. Peers and humans never see it: it is
    /// the model's private working, not part of the transcript.
    pub reasoning: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConversationToolCall {
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub status: Option<String>,
    pub args_summary: Option<String>,
    pub result_summary: Option<String>,
    pub args: Option<Value>,
    pub result: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ConversationRow {
    id: String,
    sender_type: String,
    sender_id: Option<String>,
    content: Option<String>,
    agent_name: Option<String>,
    group_agent_display_name: Option<String>,
    human_display_name: Option<String>,
    turn_id: Option<String>,
    dispatch_id: Option<String>,
    reply_to_message_id: Option<String>,
    content_json: Option<String>,
}

/// Load visible transcript rows in their durable per-thread sequence order.
pub async fn load_conversation(
    pool: &SqlitePool,
    thread_id: &str,
) -> anyhow::Result<Vec<ConversationMessage>> {
    load_conversation_rows(pool, thread_id, None).await
}

/// Load visible transcript rows plus one interrupted message being resumed.
pub(crate) async fn load_conversation_for_resume(
    pool: &SqlitePool,
    thread_id: &str,
    interrupted_message_id: &str,
) -> anyhow::Result<Vec<ConversationMessage>> {
    load_conversation_rows(pool, thread_id, Some(interrupted_message_id)).await
}

async fn load_conversation_rows(
    pool: &SqlitePool,
    thread_id: &str,
    included_message_id: Option<&str>,
) -> anyhow::Result<Vec<ConversationMessage>> {
    let rows: Vec<ConversationRow> = sqlx::query_as(
        "SELECT m.id, m.sender_type, m.sender_id, m.content, \
                a.name AS agent_name, ga.display_name AS group_agent_display_name, \
                u.name AS human_display_name, m.turn_id, m.dispatch_id, \
                m.reply_to_message_id, m.content_json \
         FROM messages m \
         LEFT JOIN agents a \
           ON m.sender_type = 'agent' AND a.id = m.sender_id \
         LEFT JOIN group_agents ga \
           ON ga.group_id = m.group_id AND ga.agent_id = m.sender_id \
         LEFT JOIN users u \
           ON m.sender_type != 'agent' AND u.id = m.sender_id \
         WHERE m.thread_id = ? AND (m.status = 'visible' OR m.id = ?) \
         ORDER BY m.seq ASC",
    )
    .bind(thread_id)
    .bind(included_message_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(ConversationMessage::from).collect())
}

impl From<ConversationRow> for ConversationMessage {
    fn from(row: ConversationRow) -> Self {
        let is_agent = row.sender_type == "agent";
        let actor_id = row.sender_id.unwrap_or_default();
        let actor = if is_agent {
            let display_name = non_empty(row.group_agent_display_name)
                .or_else(|| non_empty(row.agent_name))
                .unwrap_or_else(|| fallback_display_name(&actor_id, "Unknown Agent"));
            ConversationActor::Agent {
                id: actor_id,
                display_name,
            }
        } else {
            let display_name = non_empty(row.human_display_name)
                .unwrap_or_else(|| fallback_display_name(&actor_id, "Unknown Human"));
            ConversationActor::Human {
                id: actor_id,
                display_name,
            }
        };

        Self {
            id: row.id,
            actor,
            content: row.content.unwrap_or_default(),
            turn_id: row.turn_id,
            dispatch_id: row.dispatch_id,
            reply_to_message_id: row.reply_to_message_id,
            attachments: if is_agent {
                Vec::new()
            } else {
                attachments_from_content_json(row.content_json.as_deref())
            },
            tool_calls: if is_agent {
                tool_calls_from_content_json(row.content_json.as_deref())
            } else {
                Vec::new()
            },
            reasoning: if is_agent {
                reasoning_from_content_json(row.content_json.as_deref())
            } else {
                Vec::new()
            },
        }
    }
}

fn attachments_from_content_json(content_json: Option<&str>) -> Vec<ConversationAttachment> {
    #[derive(Deserialize)]
    struct AttachmentPayload {
        version: i64,
        #[serde(default)]
        attachments: Vec<ConversationAttachment>,
    }
    content_json
        .and_then(|raw| serde_json::from_str::<AttachmentPayload>(raw).ok())
        .filter(|payload| payload.version == 1)
        .map(|payload| payload.attachments)
        .unwrap_or_default()
}

fn tool_calls_from_content_json(content_json: Option<&str>) -> Vec<ConversationToolCall> {
    #[derive(Deserialize)]
    struct AgentPayload {
        #[serde(default)]
        tool_calls: Vec<ConversationToolCall>,
    }
    content_json
        .and_then(|raw| serde_json::from_str::<AgentPayload>(raw).ok())
        .map(|payload| payload.tool_calls)
        .unwrap_or_default()
}

fn reasoning_from_content_json(content_json: Option<&str>) -> Vec<String> {
    #[derive(Deserialize)]
    struct ReasoningPayload {
        #[serde(default)]
        reasoning: Vec<String>,
    }
    content_json
        .and_then(|raw| serde_json::from_str::<ReasoningPayload>(raw).ok())
        .map(|payload| payload.reasoning)
        .unwrap_or_default()
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn fallback_display_name(id: &str, unknown: &str) -> String {
    if id.is_empty() {
        unknown.to_string()
    } else {
        id.to_string()
    }
}

/// A rendered conversation, plus where each input row ended up.
pub struct RenderedConversation {
    pub messages: Vec<ChatMessage>,
    /// Index into [`Self::messages`] of the message carrying row `i`'s own
    /// text, when the row produced one.
    ///
    /// Rows do not map one-to-one onto messages. The current agent's own turn
    /// expands into an assistant tool-call message, one `tool` message per
    /// result, and a trailing text message — or, for an interrupted turn that
    /// produced calls but no text, into nothing at all. A caller that wants to
    /// reach back into `messages` for a given row therefore has to be told
    /// where it landed: deriving the position from the row index is off by
    /// however many extra messages the rows before it expanded into, and
    /// writing through that offset overwrites somebody else's message.
    pub message_index_by_row: Vec<Option<usize>>,
}

pub fn to_llm_messages(
    system_prompt: &str,
    current_agent_id: &str,
    rows: &[ConversationMessage],
    access: AttachmentAccess,
) -> Vec<ChatMessage> {
    render_conversation(system_prompt, current_agent_id, rows, access).messages
}

pub fn render_conversation(
    system_prompt: &str,
    current_agent_id: &str,
    rows: &[ConversationMessage],
    access: AttachmentAccess,
) -> RenderedConversation {
    let mut messages = vec![ChatMessage::text("system", system_prompt.to_string())];
    let mut message_index_by_row = Vec::with_capacity(rows.len());
    for row in rows {
        // Recorded before the row is rendered so it names the message this row
        // is about to produce, and left `None` when it produces none.
        let mut own_message_index = messages.len();
        let mut produced_own_message = true;
        match &row.actor {
            ConversationActor::Agent { id, .. } if id == current_agent_id => {
                let completed_calls: Vec<(ToolCall, String)> = row
                    .tool_calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.tool_call_id.clone()?;
                        let name = call.tool_name.clone()?;
                        let result = call.result.as_ref().or(call.result_summary.as_ref())?;
                        let args = call.args.clone().unwrap_or_else(|| {
                            call.args_summary
                                .as_deref()
                                .and_then(|raw| serde_json::from_str(raw).ok())
                                .unwrap_or_else(|| {
                                    Value::String(call.args_summary.clone().unwrap_or_default())
                                })
                        });
                        let result = call.status.as_deref().map_or_else(
                            || result.clone(),
                            |status| format!("status: {status}\n{result}"),
                        );
                        Some((
                            ToolCall {
                                id,
                                name,
                                args,
                                provider_metadata: None,
                            },
                            result,
                        ))
                    })
                    .collect();
                if !completed_calls.is_empty() {
                    // The thinking rides on the tool-call message rather than
                    // the trailing text one: that is the message a provider in
                    // thinking mode validates, and it is where the reasoning
                    // belonged in the first place.
                    messages.push(
                        ChatMessage::assistant_tool_calls(
                            "",
                            completed_calls
                                .iter()
                                .map(|(call, _)| call.clone())
                                .collect(),
                        )
                        .with_reasoning(row.reasoning.join("\n")),
                    );
                    messages.extend(completed_calls.into_iter().map(|(call, result)| {
                        ChatMessage::tool_result(call.id, call.name, result)
                    }));
                }
                if !row.content.is_empty() || row.tool_calls.is_empty() {
                    own_message_index = messages.len();
                    messages.push(ChatMessage::text("assistant", row.content.clone()));
                } else {
                    produced_own_message = false;
                }
            }
            _ => messages.push(ChatMessage::text(
                "user",
                render_untrusted_message(row, access),
            )),
        }
        message_index_by_row.push(produced_own_message.then_some(own_message_index));
    }
    RenderedConversation {
        messages,
        message_index_by_row,
    }
}

/// Render the complete ACP task while retaining the existing host task
/// envelope and current-message split.
pub fn to_acp_prompt(
    system_prompt: &str,
    current_agent_id: &str,
    rows: &[ConversationMessage],
    access: AttachmentAccess,
) -> String {
    let current_human_index = rows.last().and_then(|row| {
        matches!(row.actor, ConversationActor::Human { .. }).then_some(rows.len() - 1)
    });

    let mut prompt = String::new();
    prompt.push_str("<ag-swarmer-task>\n");
    prompt.push_str(
        "This is host-provided task context for the external ACP agent runtime; it is not the ACP runtime native system prompt.\n\n",
    );
    prompt.push_str("<agent-brief>\n");
    prompt.push_str(&escape_xml(&sanitize_acp_agent_brief(system_prompt)));
    prompt.push_str("\n</agent-brief>\n\n");

    prompt.push_str("<conversation untrusted=\"true\">\n");
    for (index, row) in rows.iter().enumerate() {
        if Some(index) == current_human_index {
            continue;
        }
        prompt.push_str(&render_acp_history_message(row, current_agent_id, access));
        prompt.push('\n');
    }
    prompt.push_str("</conversation>\n\n");

    if let Some(index) = current_human_index {
        prompt.push_str("<current-message>\n");
        prompt.push_str(&render_untrusted_message(&rows[index], access));
        prompt.push_str("\n</current-message>\n");
    }

    prompt.push_str("</ag-swarmer-task>\n");
    prompt
}

/// Render everything an existing ACP session has not been shown yet. The same
/// identity-bearing envelope used by the full transcript is retained.
///
/// The agent's own last message is the high-water mark of what its session
/// already contains, so every human and peer message after it has to travel —
/// not just the latest one. Peers speaking in turns this agent sat out are the
/// common case, and dropping them left the agent answering a reply it never
/// saw. With no message of its own to anchor on, only the latest message is
/// sent: the session was opened from the full transcript, and re-sending that
/// transcript would duplicate it.
pub fn to_acp_incremental_prompt(
    current_agent_id: &str,
    rows: &[ConversationMessage],
    access: AttachmentAccess,
) -> String {
    let undelivered: Vec<&ConversationMessage> = rows
        .iter()
        .rposition(|row| is_current_agent(row, current_agent_id))
        .map(|last_own| {
            rows[last_own + 1..]
                .iter()
                .filter(|row| !is_current_agent(row, current_agent_id))
                .collect()
        })
        .filter(|pending: &Vec<&ConversationMessage>| !pending.is_empty())
        .unwrap_or_else(|| {
            rows.iter()
                .rfind(|row| !is_current_agent(row, current_agent_id))
                .into_iter()
                .collect()
        });

    let (earlier, current) = match undelivered.split_last() {
        Some((current, earlier)) => (earlier, render_untrusted_message(current, access)),
        None => (&[][..], String::new()),
    };
    let preceding = if earlier.is_empty() {
        String::new()
    } else {
        let messages = earlier
            .iter()
            .map(|row| render_untrusted_message(row, access))
            .collect::<Vec<_>>()
            .join("\n");
        format!("<conversation untrusted=\"true\">\n{messages}\n</conversation>\n")
    };

    format!(
        "<ag-swarmer-message>\n{preceding}<current-message>\n{current}\n</current-message>\n</ag-swarmer-message>\n"
    )
}

fn is_current_agent(row: &ConversationMessage, current_agent_id: &str) -> bool {
    matches!(
        &row.actor,
        ConversationActor::Agent { id, .. } if id == current_agent_id
    )
}

fn render_acp_history_message(
    row: &ConversationMessage,
    current_agent_id: &str,
    access: AttachmentAccess,
) -> String {
    if is_current_agent(row, current_agent_id) {
        format!("assistant: {}", escape_xml(&row.content))
    } else {
        render_untrusted_message(row, access)
    }
}

fn render_untrusted_message(row: &ConversationMessage, access: AttachmentAccess) -> String {
    let (actor_type, actor_id, display_name) = match &row.actor {
        ConversationActor::Human { id, display_name } => ("human", id, display_name),
        ConversationActor::Agent { id, display_name } => ("agent", id, display_name),
    };

    let attachment_references = render_attachment_references(&row.attachments, access);
    format!(
        "<conversation-message actor_type=\"{actor_type}\" actor_id=\"{}\" display_name=\"{}\">{}{attachment_references}</conversation-message>",
        escape_xml(actor_id),
        escape_xml(display_name),
        escape_xml(&row.content),
    )
}

fn render_attachment_references(
    attachments: &[ConversationAttachment],
    access: AttachmentAccess,
) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let entries = attachments
        .iter()
        .map(|attachment| render_attachment_reference(attachment, access))
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n<workspace-attachments>\n{entries}\n</workspace-attachments>")
}

/// Render one attachment. An unreachable attachment carries no `path`: the
/// relative path is meaningful only inside the conversation workspace, and an
/// agent addressing a different root would resolve it to the wrong file.
fn render_attachment_reference(
    attachment: &ConversationAttachment,
    access: AttachmentAccess,
) -> String {
    let is_image = attachment.mime_type.starts_with("image/");
    let common = format!(
        "name=\"{}\" mime_type=\"{}\" size=\"{}\"",
        escape_xml(&attachment.name),
        escape_xml(&attachment.mime_type),
        attachment.size,
    );
    match access {
        AttachmentAccess::Unreachable => {
            let instruction = if is_image {
                "This image was shared in the conversation workspace, which your isolated workspace cannot address. Judge it only from a separately supplied native image input; never infer its content from this metadata."
            } else {
                "This file lives in the conversation workspace, which your isolated workspace cannot address. Do not guess its contents; say you cannot read it, or ask for it to be shared another way."
            };
            format!(
                "<workspace-attachment {common} accessible=\"false\">{instruction}</workspace-attachment>"
            )
        }
        AttachmentAccess::Readable => {
            let instruction = if is_image {
                "Image pixels are not represented by this metadata. Make visual or OCR claims only from a separately supplied native image input; never infer image content from its name, path, or metadata."
            } else {
                "Use workspace tools to read this file when its contents are needed."
            };
            format!(
                "<workspace-attachment {common} path=\"{}\">{instruction}</workspace-attachment>",
                escape_xml(&attachment.path)
            )
        }
    }
}

pub(crate) fn sanitize_acp_agent_brief(system_prompt: &str) -> String {
    let mut in_system_reminder = false;
    let mut lines = Vec::new();
    for line in system_prompt.lines() {
        let trimmed = line.trim();
        if trimmed.contains("<system-reminder") {
            in_system_reminder = !trimmed.contains("</system-reminder>");
            continue;
        }
        if in_system_reminder {
            if trimmed.contains("</system-reminder>") {
                in_system_reminder = false;
            }
            continue;
        }
        if trimmed.starts_with("Enabled provider-native tools:")
            || trimmed
                == "Only provider-native tool calls listed above may execute. Literal XML or pseudo-tool text is not executable tool work."
        {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_string()
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_row(content_json: Option<&str>) -> ConversationMessage {
        ConversationRow {
            id: "m1".to_string(),
            sender_type: "agent".to_string(),
            sender_id: Some("agent-1".to_string()),
            content: Some("done".to_string()),
            agent_name: Some("Helper".to_string()),
            group_agent_display_name: None,
            human_display_name: None,
            turn_id: None,
            dispatch_id: None,
            reply_to_message_id: None,
            content_json: content_json.map(str::to_string),
        }
        .into()
    }

    fn turn_json() -> &'static str {
        r#"{
            "schema_version": 1,
            "reasoning": ["first I check the file", "then I read it"],
            "tool_calls": [{
                "tool_call_id": "call_1",
                "tool_name": "Read",
                "status": "completed",
                "args": {"file_path": "note.txt"},
                "result": "file body"
            }]
        }"#
    }

    fn human_row(id: &str, content: &str) -> ConversationMessage {
        ConversationRow {
            id: id.to_string(),
            sender_type: "human".to_string(),
            sender_id: Some("user-1".to_string()),
            content: Some(content.to_string()),
            agent_name: None,
            group_agent_display_name: None,
            human_display_name: Some("Ada".to_string()),
            turn_id: None,
            dispatch_id: None,
            reply_to_message_id: None,
            content_json: None,
        }
        .into()
    }

    /// The mapping is what the vision path writes image parts through. Deriving
    /// the position arithmetically (`row index + 1`) lands on the `tool` result
    /// the agent turn expanded into, and replacing that with a user message
    /// orphans the assistant tool call above it.
    #[test]
    fn a_row_that_expands_into_several_messages_does_not_shift_later_rows() {
        let rows = vec![
            human_row("h1", "read the note"),
            agent_row(Some(turn_json())),
            human_row("h2", "now describe this image"),
        ];

        let rendered = render_conversation("sys", "agent-1", &rows, AttachmentAccess::Readable);

        // system, user(h1), assistant(calls), tool, assistant("done"), user(h2)
        assert_eq!(rendered.messages.len(), 6);
        assert_eq!(
            rendered.message_index_by_row,
            vec![Some(1), Some(4), Some(5)]
        );
        let target = rendered.message_index_by_row[2].unwrap();
        assert_eq!(rendered.messages[target].role, "user");
        assert!(rendered.messages[target]
            .content
            .contains("now describe this image"));
        assert_ne!(
            rendered.messages[2 + 1].role,
            "user",
            "the naive offset lands on the tool result, which is the bug"
        );
    }

    /// An interrupted turn — calls recorded, none completed, no text — renders
    /// as nothing at all. A caller indexing by row would run off the end.
    #[test]
    fn a_row_that_renders_to_nothing_maps_to_none() {
        let rows = vec![agent_row(Some(
            r#"{"schema_version":1,"tool_calls":[{"tool_call_id":"c","tool_name":"Read"}]}"#,
        ))];
        let mut rows = rows;
        rows[0].content = String::new();

        let rendered = render_conversation("sys", "agent-1", &rows, AttachmentAccess::Readable);

        assert_eq!(rendered.messages.len(), 1, "only the system prompt");
        assert_eq!(rendered.message_index_by_row, vec![None]);
    }

    #[test]
    fn a_rehydrated_tool_call_carries_the_thinking_that_produced_it() {
        let rows = vec![agent_row(Some(turn_json()))];

        let messages = to_llm_messages("sys", "agent-1", &rows, AttachmentAccess::Readable);

        let assistant = messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .expect("the completed call is replayed as an assistant message");
        // Without this, resuming a paused turn rebuilds the tool-calling message
        // from the transcript with its reasoning dropped, and a provider in
        // thinking mode rejects the continuation outright.
        assert_eq!(
            assistant.reasoning_content.as_deref(),
            Some("first I check the file\nthen I read it")
        );
        assert!(
            messages
                .iter()
                .filter(|message| message.reasoning_content.is_some())
                .count()
                == 1,
            "reasoning belongs to the one message that made the call"
        );
    }

    #[test]
    fn a_turn_with_no_recorded_reasoning_attaches_none() {
        let rows = vec![agent_row(Some(
            r#"{"schema_version":1,"tool_calls":[{"tool_call_id":"c","tool_name":"Read","status":"completed","args":{},"result":"x"}]}"#,
        ))];

        let messages = to_llm_messages("sys", "agent-1", &rows, AttachmentAccess::Readable);

        let assistant = messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .expect("the completed call is replayed");
        assert_eq!(assistant.reasoning_content, None);
    }

    #[test]
    fn a_peers_reasoning_never_leaks_into_this_agents_history() {
        let rows = vec![agent_row(Some(turn_json()))];

        // Same rows, read by a different agent: the turn is someone else's, so
        // it renders as an untrusted conversation message with no thinking.
        let messages = to_llm_messages("sys", "agent-2", &rows, AttachmentAccess::Readable);

        assert!(messages
            .iter()
            .all(|message| message.reasoning_content.is_none()));
        assert!(messages.iter().all(|message| message.tool_calls.is_empty()));
    }
}
