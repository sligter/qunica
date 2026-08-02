use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::llm::{build_provider, ChatDelta, ChatMessage, ChatRequest, ProviderConfig};

pub const MAX_OBJECTIVE_CHARS: usize = 2_000;
pub const MAX_RECENT_MESSAGES: usize = 4;
pub const MAX_MESSAGE_CHARS: usize = 1_000;

const SYSTEM_INSTRUCTION: &str = "You are a private group scheduler moderator. Select exactly one candidate agent_id from the provided candidates. Respond with JSON only in the form {\"agent_id\":\"...\"}.";

#[derive(Debug, Clone)]
pub struct ModeratorConfig {
    pub owner_id: String,
    pub provider_id: String,
    pub model: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModeratorCandidate {
    pub agent_id: String,
    pub display_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModeratorMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ModeratorRequest {
    pub objective: String,
    pub recent_messages: Vec<ModeratorMessage>,
    pub candidates: Vec<ModeratorCandidate>,
    pub remaining_steps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeratorSelection {
    pub agent_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeratorFailure {
    MissingConfiguration,
    ProviderUnavailable,
    Provider,
    Timeout,
    InvalidResponse,
    UnexpectedDelta,
}

impl ModeratorFailure {
    /// Stable wire name reported alongside a `moderator_fallback` event so a
    /// deleted provider or a misbehaving model is distinguishable from a plain
    /// call-budget fallback.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingConfiguration => "missing_configuration",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Provider => "provider_error",
            Self::Timeout => "timeout",
            Self::InvalidResponse => "invalid_response",
            Self::UnexpectedDelta => "unexpected_delta",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeratorAttempt {
    pub result: Result<ModeratorSelection, ModeratorFailure>,
    pub provider_called: bool,
    pub total_tokens: u64,
}

#[derive(sqlx::FromRow)]
struct ProviderRow {
    kind: String,
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    reasoning_passback: i64,
    context_window_tokens: Option<i64>,
    context_output_reserve_ratio: Option<f64>,
    models_json: Option<String>,
}

pub async fn select_with_moderator(
    pool: &SqlitePool,
    config: &ModeratorConfig,
    request: ModeratorRequest,
) -> ModeratorAttempt {
    if config.owner_id.is_empty() || config.provider_id.is_empty() || config.model.is_empty() {
        return failed(ModeratorFailure::MissingConfiguration, false, 0);
    }

    let provider_cfg = match resolve_provider(pool, config).await {
        Ok(config) => config,
        Err(failure) => return failed(failure, false, 0),
    };
    let provider = match build_provider(&provider_cfg) {
        Ok(provider) => provider,
        Err(_) => return failed(ModeratorFailure::Provider, false, 0),
    };
    let request = bounded_request(request);
    let candidates = request.candidates.clone();
    let request = moderator_chat_request(config, &request);

    let deadline = tokio::time::Instant::now() + config.timeout;
    let provider_called = true;
    let mut deltas = match tokio::time::timeout_at(deadline, provider.stream(request)).await {
        Ok(Ok(deltas)) => deltas,
        Ok(Err(_)) => return failed(ModeratorFailure::Provider, provider_called, 0),
        Err(_) => return failed(ModeratorFailure::Timeout, provider_called, 0),
    };
    match collect_response(&mut deltas, deadline).await {
        Ok((response, total_tokens)) => match parse_selection(&response, &candidates) {
            Ok(selection) => ModeratorAttempt {
                result: Ok(selection),
                provider_called,
                total_tokens,
            },
            Err(failure) => failed(failure, provider_called, total_tokens),
        },
        Err((failure, total_tokens)) => failed(failure, provider_called, total_tokens),
    }
}

async fn resolve_provider(
    pool: &SqlitePool,
    config: &ModeratorConfig,
) -> Result<ProviderConfig, ModeratorFailure> {
    let row: Option<ProviderRow> = sqlx::query_as(
        "SELECT kind, base_url, api_key, default_model, reasoning_passback, \
                context_window_tokens, context_output_reserve_ratio, models_json \
        FROM llm_providers WHERE id = ? AND owner_id = ? AND status = 'active'",
    )
    .bind(&config.provider_id)
    .bind(&config.owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ModeratorFailure::ProviderUnavailable)?;
    let row = row.ok_or(ModeratorFailure::ProviderUnavailable)?;
    let (model_window, model_reserve) =
        crate::llm::model_context_config(row.models_json.as_deref(), &config.model);
    let reasoning_passback = crate::llm::model_reasoning_passback(
        row.models_json.as_deref(),
        &config.model,
        row.reasoning_passback != 0,
    );

    Ok(ProviderConfig {
        kind: row.kind,
        base_url: row.base_url,
        api_key: row.api_key,
        default_model: row.default_model,
        reasoning_passback,
        context_window_tokens: model_window.or(row.context_window_tokens),
        context_output_reserve_ratio: model_reserve.or(row.context_output_reserve_ratio),
    })
}

fn moderator_chat_request(config: &ModeratorConfig, request: &ModeratorRequest) -> ChatRequest {
    ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage::text("system", SYSTEM_INSTRUCTION),
            ChatMessage::text(
                "user",
                json!({
                    "objective": request.objective,
                    "recent_messages": request.recent_messages,
                    "candidates": request.candidates,
                    "remaining_steps": request.remaining_steps,
                })
                .to_string(),
            ),
        ],
        temperature: Some(0.0),
        reasoning_passback: false,
        include_empty_tools: true,
        tools: Vec::new(),
        // The moderator picks a speaker; the user's per-message choice is
        // about the answer, not about routing.
        reasoning_effort: None,
    }
}

async fn collect_response(
    deltas: &mut tokio::sync::mpsc::Receiver<ChatDelta>,
    deadline: tokio::time::Instant,
) -> Result<(String, u64), (ModeratorFailure, u64)> {
    let mut response = String::new();
    let mut total_tokens: u64 = 0;

    loop {
        let delta = match tokio::time::timeout_at(deadline, deltas.recv()).await {
            Ok(Some(delta)) => delta,
            Ok(None) => return Err((ModeratorFailure::Provider, total_tokens)),
            Err(_) => return Err((ModeratorFailure::Timeout, total_tokens)),
        };
        match delta {
            ChatDelta::Token(fragment) => response.push_str(&fragment),
            ChatDelta::Usage(usage) => {
                let tokens = usage.total_tokens.or(usage.output_tokens).unwrap_or(0);
                total_tokens = total_tokens.saturating_add(u64::try_from(tokens).unwrap_or(0));
            }
            ChatDelta::Done => break,
            ChatDelta::Reasoning(_) | ChatDelta::ToolCall(_) => {
                return Err((ModeratorFailure::UnexpectedDelta, total_tokens));
            }
        }
    }

    Ok((response, total_tokens))
}

fn bounded_request(request: ModeratorRequest) -> ModeratorRequest {
    let message_start = request
        .recent_messages
        .len()
        .saturating_sub(MAX_RECENT_MESSAGES);
    ModeratorRequest {
        objective: truncate(&request.objective, MAX_OBJECTIVE_CHARS),
        recent_messages: request.recent_messages[message_start..]
            .iter()
            .map(|message| ModeratorMessage {
                role: truncate(&message.role, MAX_MESSAGE_CHARS),
                content: truncate(&message.content, MAX_MESSAGE_CHARS),
            })
            .collect(),
        candidates: request
            .candidates
            .iter()
            .map(|candidate| ModeratorCandidate {
                agent_id: truncate(&candidate.agent_id, MAX_MESSAGE_CHARS),
                display_name: truncate(&candidate.display_name, MAX_MESSAGE_CHARS),
                reason: truncate(&candidate.reason, MAX_MESSAGE_CHARS),
            })
            .collect(),
        remaining_steps: request.remaining_steps,
    }
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn parse_selection(
    response: &str,
    candidates: &[ModeratorCandidate],
) -> Result<ModeratorSelection, ModeratorFailure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SelectionResponse {
        agent_id: String,
    }

    let response: SelectionResponse =
        serde_json::from_str(response).map_err(|_| ModeratorFailure::InvalidResponse)?;
    if candidates
        .iter()
        .any(|candidate| candidate.agent_id == response.agent_id)
    {
        Ok(ModeratorSelection {
            agent_id: response.agent_id,
        })
    } else {
        Err(ModeratorFailure::InvalidResponse)
    }
}

fn failed(failure: ModeratorFailure, provider_called: bool, total_tokens: u64) -> ModeratorAttempt {
    ModeratorAttempt {
        result: Err(failure),
        provider_called,
        total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_request, collect_response, moderator_chat_request, parse_selection,
        resolve_provider, ModeratorCandidate, ModeratorConfig, ModeratorFailure, ModeratorMessage,
        ModeratorRequest, MAX_MESSAGE_CHARS, MAX_OBJECTIVE_CHARS, MAX_RECENT_MESSAGES,
    };
    use crate::{db::Db, llm::ChatDelta};
    use serde_json::Value;
    use std::time::Duration;

    #[test]
    fn moderator_response_requires_one_known_agent_id() {
        let candidates = vec![ModeratorCandidate {
            agent_id: "a".to_owned(),
            display_name: "Alpha".to_owned(),
            reason: "eligible".to_owned(),
        }];
        assert_eq!(
            parse_selection(r#"{"agent_id":"a"}"#, &candidates)
                .unwrap()
                .agent_id,
            "a"
        );
        assert!(parse_selection(r#"{"agent_id":"a","reason":"x"}"#, &candidates).is_err());
        assert!(parse_selection(r#"{"agent_id":7}"#, &candidates).is_err());
        assert!(parse_selection(r#"{"agent_id":"missing"}"#, &candidates).is_err());
    }

    #[test]
    fn moderator_request_bounds_unicode_text_and_recent_messages() {
        let request = ModeratorRequest {
            objective: "a".repeat(MAX_OBJECTIVE_CHARS - 1) + "\u{1f680}\u{1f680}",
            recent_messages: (0..=MAX_RECENT_MESSAGES)
                .map(|index| ModeratorMessage {
                    role: format!("role-{index}"),
                    content: "\u{1f680}".repeat(MAX_MESSAGE_CHARS + index),
                })
                .collect(),
            candidates: Vec::new(),
            remaining_steps: 3,
        };

        let bounded = bounded_request(request);
        assert_eq!(bounded.objective.chars().count(), MAX_OBJECTIVE_CHARS);
        assert_eq!(bounded.recent_messages.len(), MAX_RECENT_MESSAGES);
        assert_eq!(bounded.recent_messages[0].role, "role-1");
        assert_eq!(
            bounded.recent_messages[0].content.chars().count(),
            MAX_MESSAGE_CHARS
        );
    }

    #[test]
    fn moderator_chat_request_uses_explicit_model_and_bounded_json_input() {
        let config = ModeratorConfig {
            owner_id: "owner".to_owned(),
            provider_id: "provider".to_owned(),
            model: "configured-moderator-model".to_owned(),
            timeout: Duration::from_secs(1),
        };
        let request = bounded_request(ModeratorRequest {
            objective: "objective".to_owned(),
            recent_messages: vec![ModeratorMessage {
                role: "user".to_owned(),
                content: "message".to_owned(),
            }],
            candidates: vec![ModeratorCandidate {
                agent_id: "a".to_owned(),
                display_name: "Alpha".to_owned(),
                reason: "eligible".to_owned(),
            }],
            remaining_steps: 2,
        });

        let chat = moderator_chat_request(&config, &request);
        assert_eq!(chat.model, "configured-moderator-model");
        assert_eq!(chat.temperature, Some(0.0));
        assert!(!chat.reasoning_passback);
        assert!(chat.include_empty_tools);
        assert!(chat.tools.is_empty());
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.messages[0].role, "system");
        let payload: Value = serde_json::from_str(&chat.messages[1].content).unwrap();
        assert_eq!(payload["objective"], "objective");
        assert_eq!(payload["recent_messages"][0]["content"], "message");
        assert_eq!(payload["candidates"][0]["agent_id"], "a");
        assert_eq!(payload["remaining_steps"], 2);
    }

    #[tokio::test]
    async fn moderator_resolves_only_an_active_provider_owned_by_the_moderator_owner() {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        let pool = db.pool();
        sqlx::query("INSERT INTO users (id, email, password_hash, name, created_at, updated_at) VALUES ('owner', 'owner@example.test', 'hash', 'Owner', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO llm_providers (id, owner_id, name, kind, base_url, api_key, default_model, reasoning_passback, status, created_at, updated_at) VALUES ('provider', 'owner', 'Moderator', 'openai', 'https://provider.example.test', 'key', 'provider-default', 1, 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(pool)
            .await
            .unwrap();
        let config = ModeratorConfig {
            owner_id: "owner".to_owned(),
            provider_id: "provider".to_owned(),
            model: "configured-model".to_owned(),
            timeout: Duration::from_secs(1),
        };

        let provider = resolve_provider(pool, &config).await.unwrap();
        assert_eq!(provider.kind, "openai");
        assert_eq!(provider.default_model, "provider-default");
        assert!(provider.reasoning_passback);

        let wrong_owner = ModeratorConfig {
            owner_id: "other-owner".to_owned(),
            ..config
        };
        assert!(matches!(
            resolve_provider(pool, &wrong_owner).await,
            Err(ModeratorFailure::ProviderUnavailable)
        ));
    }

    #[tokio::test]
    async fn moderator_stream_requires_done_before_parsing_selection() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        sender
            .send(ChatDelta::Token(r#"{"agent_id":"a"}"#.to_owned()))
            .await
            .unwrap();
        drop(sender);

        assert_eq!(
            collect_response(
                &mut receiver,
                tokio::time::Instant::now() + Duration::from_secs(1),
            )
            .await,
            Err((ModeratorFailure::Provider, 0))
        );
    }
}
