//! Identity-aware group conversation loading and provider prompt rendering.
//!
//! Message content from humans and peer agents is untrusted. Renderers preserve
//! the speaker identity in an escaped host-controlled envelope; only the
//! current agent's own history is represented as assistant output.

use crate::llm::ChatMessage;
use serde::Deserialize;
use sqlx::SqlitePool;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConversationAttachment {
    pub id: String,
    pub path: String,
    pub name: String,
    pub mime_type: String,
    pub size: i64,
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
            attachments: (!is_agent)
                .then(|| attachments_from_content_json(row.content_json.as_deref()))
                .unwrap_or_default(),
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

pub fn to_llm_messages(
    system_prompt: &str,
    current_agent_id: &str,
    rows: &[ConversationMessage],
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::text("system", system_prompt.to_string())];
    messages.extend(rows.iter().map(|row| match &row.actor {
        ConversationActor::Agent { id, .. } if id == current_agent_id => {
            ChatMessage::text("assistant", row.content.clone())
        }
        _ => ChatMessage::text("user", render_untrusted_message(row)),
    }));
    messages
}

/// Render the complete ACP task while retaining the existing host task
/// envelope and current-message split.
pub fn to_acp_prompt(
    system_prompt: &str,
    current_agent_id: &str,
    rows: &[ConversationMessage],
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
        prompt.push_str(&render_acp_history_message(row, current_agent_id));
        prompt.push('\n');
    }
    prompt.push_str("</conversation>\n\n");

    if let Some(index) = current_human_index {
        prompt.push_str("<current-message>\n");
        prompt.push_str(&render_untrusted_message(&rows[index]));
        prompt.push_str("\n</current-message>\n");
    }

    prompt.push_str("</ag-swarmer-task>\n");
    prompt
}

/// Render the latest non-self message for an existing ACP session. The same
/// identity-bearing envelope used by the full transcript is retained.
pub fn to_acp_incremental_prompt(current_agent_id: &str, rows: &[ConversationMessage]) -> String {
    let current_message = rows
        .iter()
        .rfind(|row| !is_current_agent(row, current_agent_id))
        .map(render_untrusted_message)
        .unwrap_or_default();

    format!(
        "<ag-swarmer-message>\n<current-message>\n{current_message}\n</current-message>\n</ag-swarmer-message>\n"
    )
}

fn is_current_agent(row: &ConversationMessage, current_agent_id: &str) -> bool {
    matches!(
        &row.actor,
        ConversationActor::Agent { id, .. } if id == current_agent_id
    )
}

fn render_acp_history_message(row: &ConversationMessage, current_agent_id: &str) -> String {
    if is_current_agent(row, current_agent_id) {
        format!("assistant: {}", escape_xml(&row.content))
    } else {
        render_untrusted_message(row)
    }
}

fn render_untrusted_message(row: &ConversationMessage) -> String {
    let (actor_type, actor_id, display_name) = match &row.actor {
        ConversationActor::Human { id, display_name } => ("human", id, display_name),
        ConversationActor::Agent { id, display_name } => ("agent", id, display_name),
    };

    let attachment_references = render_attachment_references(&row.attachments);
    format!(
        "<conversation-message actor_type=\"{actor_type}\" actor_id=\"{}\" display_name=\"{}\">{}{attachment_references}</conversation-message>",
        escape_xml(actor_id),
        escape_xml(display_name),
        escape_xml(&row.content),
    )
}

fn render_attachment_references(attachments: &[ConversationAttachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let entries = attachments.iter().map(|attachment| format!(
        "<workspace-attachment name=\"{}\" mime_type=\"{}\" size=\"{}\" path=\"{}\">Use workspace tools to read this file. Native image input may be available separately.</workspace-attachment>",
        escape_xml(&attachment.name), escape_xml(&attachment.mime_type), attachment.size, escape_xml(&attachment.path)
    )).collect::<Vec<_>>().join("\n");
    format!("\n<workspace-attachments>\n{entries}\n</workspace-attachments>")
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
