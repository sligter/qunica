//! Assistant-generated titles for direct chats.
//!
//! A new chat starts with a placeholder title ("New chat with X", replaced by
//! a truncated first message once one exists). When the opening message of a
//! direct chat arrives, this module asks the conversation's own agent —
//! through the same provider resolution the agent's replies use — to name the
//! chat instead, and the result is announced on the same stream as a second
//! `conversation_updated` event.
//!
//! Everything here is best effort by construction: any failure resolves to
//! `None`, leaving the placeholder untouched, and the final write is
//! conditional on `title_source = 'automatic'` so a manual rename racing this
//! code simply wins.

use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;

use crate::llm::{
    build_provider, model_from_config, ChatDelta, ChatMessage, ChatRequest, ProviderConfig,
};

/// Hard cap on a generated title, counted in characters rather than bytes so
/// CJK titles are not cut four times shorter than Latin ones.
pub(crate) const MAX_TITLE_CHARS: usize = 60;

/// Whole-operation budget covering the provider connection and the answer.
///
/// This runs before the reply starts, so a stalled provider holds the turn
/// open by at most this long — once per chat, never per message.
const TITLE_TIMEOUT: Duration = Duration::from_secs(10);

/// How much of the opening message the naming prompt may see.
const USER_EXCERPT_CHARS: usize = 2000;

const TITLE_SYSTEM_PROMPT: &str = "\
You name chat conversations. Reply with the title text ONLY: no quotes, no \
label such as \"Title:\", no trailing period, at most 60 characters. Write \
the title in the same language as the message, capturing its concrete topic \
rather than greeting words.";

/// The title written into `groups.name`, with the timestamp announced to
/// clients in the `conversation_updated` payload.
pub(crate) struct GeneratedTitle {
    pub title: String,
    pub updated_at: String,
}

/// Maybe name a direct chat from its opening user message.
///
/// Skips (returning `None`) when the conversation is not a direct chat, the
/// message is not the first one, the agent has no usable provider, or
/// generation fails or yields nothing usable.
pub(crate) async fn maybe_generate_direct_chat_title(
    pool: &SqlitePool,
    group_id: &str,
) -> Option<GeneratedTitle> {
    match try_generate(pool, group_id).await {
        Ok(generated) => generated,
        Err(error) => {
            tracing::warn!(group_id, error = %error, "direct chat title generation skipped");
            None
        }
    }
}

async fn try_generate(pool: &SqlitePool, group_id: &str) -> anyhow::Result<Option<GeneratedTitle>> {
    let row: Option<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT owner_id, direct_agent_id, title_source FROM groups \
         WHERE id = ? AND status = 'active' AND conversation_kind = 'direct'",
    )
    .bind(group_id)
    .fetch_optional(pool)
    .await?;
    let Some((owner_id, agent_id, title_source)) = row else {
        return Ok(None);
    };
    // A user-authored name is final. Check before provider resolution so a
    // manually renamed empty chat neither leaks its first message to a naming
    // call nor delays the real reply only to discard the generated result.
    if title_source != "automatic" {
        return Ok(None);
    }
    let Some(agent_id) = agent_id else {
        return Ok(None);
    };

    // Naming is a first-impression job: once a second user message exists the
    // placeholder has served its purpose, and regenerating would spend a
    // provider call per message forever.
    let user_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE group_id = ? AND sender_type = 'user'",
    )
    .bind(group_id)
    .fetch_one(pool)
    .await?;
    if user_count != 1 {
        return Ok(None);
    }

    let user_content: Option<String> = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id = ? AND sender_type = 'user' \
         ORDER BY seq ASC, id ASC LIMIT 1",
    )
    .bind(group_id)
    .fetch_optional(pool)
    .await?;
    let Some(user_content) = user_content.filter(|content| !content.trim().is_empty()) else {
        return Ok(None);
    };

    let agent: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT owner_id, provider_id, model_config_json FROM agents \
         WHERE id = ? AND status = 'active'",
    )
    .bind(&agent_id)
    .fetch_optional(pool)
    .await?;
    let Some((agent_owner, provider_id, model_config_json)) = agent else {
        return Ok(None);
    };
    if agent_owner != owner_id {
        return Ok(None);
    }
    let Some(provider_id) = provider_id else {
        // External (ACP) agents and unconfigured LLM agents have no provider
        // to ask; the truncated placeholder remains.
        tracing::info!(
            group_id,
            "no provider bound; keeping placeholder chat title"
        );
        return Ok(None);
    };

    let (provider_cfg, _provider_name) = match crate::runtime::group::resolve_provider_for_binding(
        pool,
        &owner_id,
        &provider_id,
        &model_config_json,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::info!(group_id, error = %error, "provider unavailable for chat title");
            return Ok(None);
        }
    };
    let model = model_from_config(&model_config_json, &provider_cfg.default_model);

    let title = match generate_title_with_provider(&provider_cfg, &model, &user_content).await {
        Ok(title) if !title.is_empty() => title,
        Ok(_) => return Ok(None),
        Err(error) => {
            tracing::info!(group_id, error = %error, "assistant title generation failed");
            return Ok(None);
        }
    };

    // Conditional final write, deliberately without the global write lock: the
    // direct-chat rename endpoint writes the same column unlocked, and the
    // `title_source` predicate is what arbitrates against a concurrent manual
    // rename — whichever lands second simply does not apply.
    let now = now_rfc3339();
    let result = sqlx::query(
        "UPDATE groups SET name = ?, updated_at = ? \
         WHERE id = ? AND status = 'active' AND title_source = 'automatic'",
    )
    .bind(&title)
    .bind(&now)
    .bind(group_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(GeneratedTitle {
        title,
        updated_at: now,
    }))
}

/// Ask `model` for a title and return the sanitized answer.
async fn generate_title_with_provider(
    provider_cfg: &ProviderConfig,
    model: &str,
    user_content: &str,
) -> anyhow::Result<String> {
    let prompt = format!(
        "First user message:\n{}",
        excerpt(user_content, USER_EXCERPT_CHARS)
    );
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage::text("system", TITLE_SYSTEM_PROMPT),
            ChatMessage::text("user", &prompt),
        ],
        temperature: None,
        // One-shot transcription-style call: no thinking budget, no tools.
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: None,
    };
    let provider = build_provider(provider_cfg)?;

    let attempt = async {
        let deltas = provider.stream(request).await?;
        Ok::<String, anyhow::Error>(drain_text(deltas).await)
    };
    match tokio::time::timeout(TITLE_TIMEOUT, attempt).await {
        Ok(result) => Ok(sanitize_title(&result?)),
        Err(_) => anyhow::bail!("title generation timed out after {TITLE_TIMEOUT:?}"),
    }
}

/// Drain a provider stream into its text, ignoring everything else.
///
/// Usage is deliberately not recorded: the call is a one-off per chat, and
/// attributing it would widen the token-usage dimensions for little value. A
/// truncated stream still yields whatever text arrived, matching how the
/// compaction summarizer treats the same situation.
async fn drain_text(mut deltas: Receiver<ChatDelta>) -> String {
    let mut text = String::new();
    while let Some(delta) = deltas.recv().await {
        match delta {
            ChatDelta::Token(chunk) => text.push_str(&chunk),
            ChatDelta::Done => break,
            _ => {}
        }
    }
    text
}

/// Collapse a message excerpt onto one line and cap its length.
fn excerpt(content: &str, max_chars: usize) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(max_chars).collect()
}

/// Clean a model's answer down to bare title text.
///
/// Providers wrap answers in quotes surprisingly often, and smaller models
/// like announcing their work ("Title: ..."); both read badly in a sidebar.
pub(crate) fn sanitize_title(raw: &str) -> String {
    let mut current = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    // Labels and quotes can nest one deep ("Title: \"foo\""); iterate until
    // stable, bounded so a pathological answer cannot spin.
    for _ in 0..4 {
        let next = strip_title_label(&strip_wrapping_quotes(&current));
        if next == current {
            break;
        }
        current = next;
    }
    let capped: String = current.chars().take(MAX_TITLE_CHARS).collect();
    capped.trim().to_string()
}

fn strip_title_label(input: &str) -> String {
    let trimmed = input.trim();
    let lowered = trimmed.to_lowercase();
    for prefix in ["title:", "\u{6807}\u{9898}\u{ff1a}", "\u{6807}\u{9898}:"] {
        if lowered.starts_with(prefix) {
            // The prefixes are ASCII plus CJK ideographs, neither changed by
            // `to_lowercase`, so slicing the original by the prefix's byte
            // length stays on a char boundary.
            return trimmed[prefix.len()..].trim_start().to_string();
        }
    }
    trimmed.to_string()
}

/// Remove one layer of matched wrapping quotes per call.
fn strip_wrapping_quotes(input: &str) -> String {
    const PAIRS: [(char, char); 7] = [
        ('"', '"'),
        ('\'', '\''),
        ('\u{201c}', '\u{201d}'),
        ('\u{2018}', '\u{2019}'),
        ('\u{300c}', '\u{300d}'),
        ('\u{300e}', '\u{300f}'),
        ('\u{300a}', '\u{300b}'),
    ];
    let trimmed = input.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < 2 {
        return trimmed.to_string();
    }
    let first = chars[0];
    let last = chars[chars.len() - 1];
    if PAIRS
        .iter()
        .any(|(open, close)| *open == first && *close == last)
    {
        return chars[1..chars.len() - 1]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
    }
    trimmed.to_string()
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ascii_and_cjk_wrapping_quotes() {
        assert_eq!(sanitize_title("\"Rust ownership\""), "Rust ownership");
        assert_eq!(
            sanitize_title("\u{201c}\u{9519}\u{8bef}\u{5904}\u{7406}\u{201d}"),
            "\u{9519}\u{8bef}\u{5904}\u{7406}"
        );
        assert_eq!(
            sanitize_title("\u{300c}\u{6240}\u{6709}\u{6743}\u{300d}"),
            "\u{6240}\u{6709}\u{6743}"
        );
    }

    #[test]
    fn strips_title_labels_in_both_languages() {
        assert_eq!(sanitize_title("Title: Debugging async"), "Debugging async");
        assert_eq!(
            sanitize_title("\u{6807}\u{9898}\u{ff1a}\u{5f02}\u{6b65}\u{8c03}\u{8bd5}"),
            "\u{5f02}\u{6b65}\u{8c03}\u{8bd5}"
        );
    }

    #[test]
    fn strips_a_nested_label_inside_quotes() {
        assert_eq!(sanitize_title("\"Title: weekly notes\""), "weekly notes");
    }

    #[test]
    fn collapses_newlines_and_repeated_spaces() {
        assert_eq!(
            sanitize_title("Line one\n\nline  two\tthree"),
            "Line one line two three"
        );
    }

    #[test]
    fn caps_by_characters_not_bytes() {
        let cjk: String = "\u{6d4b}".repeat(80);
        assert_eq!(sanitize_title(&cjk).chars().count(), MAX_TITLE_CHARS);
        let latin: String = "a".repeat(80);
        assert_eq!(sanitize_title(&latin).chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn empty_and_blank_answers_yield_empty_titles() {
        assert_eq!(sanitize_title(""), "");
        assert_eq!(sanitize_title("   \n  "), "");
        assert_eq!(sanitize_title("\"\""), "");
    }

    #[test]
    fn excerpts_normalize_space_and_cap_length() {
        assert_eq!(excerpt("  hello   world  ", 20), "hello world");
        assert_eq!(excerpt("abcdef", 3).chars().count(), 3);
    }
}
