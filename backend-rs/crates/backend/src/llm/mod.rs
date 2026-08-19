//! LLM provider clients.
//!
//! Each provider implements [`LlmProvider`] by issuing a streaming HTTP request
//! and translating provider-specific server-sent events into the neutral
//! [`ChatDelta`] vocabulary defined by the domain runtime contract. The clients
//! here do parsing and mapping only — request orchestration, tool execution and
//! group runtime live in later tasks.

pub mod anthropic;
pub mod gemini;
pub mod model_catalog;
pub mod openai_compatible;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use model_catalog::{discover_models, ModelCatalogError, ModelInfo, MODEL_CATALOG_TIMEOUT};
pub use openai_compatible::OpenAiCompatibleProvider;

// Re-export the runtime data contract so integration tests (which link only
// against this crate) can name the shared types without depending on the domain
// crate directly. The domain crate holds only pure data types; the streaming
// provider behaviour below lives here in the backend.
pub use ag_swarmer_domain::runtime::{
    ChatDelta, ChatMessage, ChatRequest, ContextUsage, ReasoningEffort, ToolCall, ToolDefinition,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{Receiver, Sender};

/// Largest provider error body retained. Enough for any provider's JSON error;
/// short enough that an HTML error page does not flood the log or the turn.
const MAX_ERROR_BODY_CHARS: usize = 2_000;

/// A non-2xx provider response, with the body preserved.
///
/// `reqwest`'s `error_for_status` discards the body, which is exactly where
/// every provider puts the reason. That made a 400 saying "this model's maximum
/// context length is 128000 tokens" indistinguishable from a malformed request,
/// so the runtime could neither report it usefully nor recover from it — the
/// turn just failed with "provider execution failed".
#[derive(Debug, Clone, thiserror::Error)]
#[error("provider returned HTTP {status}: {body}")]
pub struct ProviderHttpError {
    pub status: u16,
    pub body: String,
}

impl ProviderHttpError {
    /// A rendering safe to show a user: the status, without the body.
    ///
    /// The body is kept on the struct for classification and server-side logs,
    /// but never travels to a client. Providers routinely echo the submitted
    /// credential back in an authentication error ("Incorrect API key provided:
    /// sk-…"), so a body rendered into a chat stream or a settings panel is a
    /// credential rendered into a chat stream or a settings panel.
    pub fn safe_message(&self) -> String {
        format!("The provider returned HTTP {}.", self.status)
    }

    /// Worth retrying with the same request: a timeout, a rate limit, or a
    /// server-side fault.
    pub fn is_transient(&self) -> bool {
        self.status == 408 || self.status == 429 || (500..600).contains(&self.status)
    }

    /// Whether the provider rejected the request for exceeding its context
    /// window.
    ///
    /// Matched on the body text because no provider signals this in a
    /// structured, portable way: OpenAI-compatible hosts use an
    /// `context_length_exceeded` code, Anthropic prose, Gemini something else
    /// again. A false negative costs a failed turn (the previous behaviour); a
    /// false positive costs one compaction that was not needed.
    pub fn is_context_overflow(&self) -> bool {
        if self.status != 400 && self.status != 413 && self.status != 422 {
            return false;
        }
        let body = self.body.to_lowercase();
        [
            "context length",
            "context_length_exceeded",
            "maximum context",
            "context window",
            "too many tokens",
            "prompt is too long",
            "input is too long",
            "reduce the length",
            "exceeds the maximum",
        ]
        .iter()
        .any(|needle| body.contains(needle))
    }
}

/// Reject a non-2xx response while keeping the provider's own explanation.
pub(crate) async fn ensure_success(
    response: reqwest::Response,
) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let body: String = if body.chars().count() > MAX_ERROR_BODY_CHARS {
        body.chars().take(MAX_ERROR_BODY_CHARS).collect()
    } else {
        body
    };
    Err(ProviderHttpError {
        status: status.as_u16(),
        body,
    }
    .into())
}

/// Resolved LLM provider connection settings.
pub struct ProviderConfig {
    pub kind: String,
    pub base_url: Option<String>,
    pub api_key: String,
    pub default_model: String,
    pub reasoning_passback: bool,
    /// Total context window size in tokens, if the provider declares one.
    pub context_window_tokens: Option<i64>,
    /// Fraction of the window reserved for model output (0.0..=1.0).
    pub context_output_reserve_ratio: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ProviderModelConfig {
    pub id: String,
    #[serde(default)]
    pub context_window_tokens: Option<i64>,
    #[serde(default)]
    pub context_output_reserve_ratio: Option<f64>,
    /// Whether this model accepts a reasoning-effort setting.
    ///
    /// Defaults to false so the control stays hidden until someone says
    /// otherwise: sending the field to a model that rejects it turns a normal
    /// question into a provider error, and models that predate this flag have
    /// no value stored.
    #[serde(default)]
    pub supports_reasoning_effort: bool,
    /// Whether prior reasoning is sent back during this model's tool loop.
    #[serde(default)]
    pub reasoning_passback: Option<bool>,
}

pub fn model_context_config(models_json: Option<&str>, model: &str) -> (Option<i64>, Option<f64>) {
    models_json
        .and_then(|raw| serde_json::from_str::<Vec<ProviderModelConfig>>(raw).ok())
        .and_then(|models| models.into_iter().find(|item| item.id == model))
        .map(|item| {
            (
                item.context_window_tokens,
                item.context_output_reserve_ratio,
            )
        })
        .unwrap_or((None, None))
}

pub fn model_reasoning_passback(
    models_json: Option<&str>,
    model: &str,
    legacy_fallback: bool,
) -> bool {
    let Some(models) =
        models_json.and_then(|raw| serde_json::from_str::<Vec<ProviderModelConfig>>(raw).ok())
    else {
        return legacy_fallback;
    };
    if models.iter().all(|item| item.reasoning_passback.is_none()) {
        return legacy_fallback;
    }
    models
        .into_iter()
        .find(|item| item.id == model)
        .and_then(|item| item.reasoning_passback)
        .unwrap_or(false)
}

/// A streaming chat completion provider.
///
/// Implementors issue a streaming HTTP request to a specific vendor API and
/// translate its wire format into the neutral [`ChatDelta`] vocabulary. This
/// trait carries the runtime dependencies (async/`tokio`/`anyhow`) and so lives
/// in the backend rather than the pure-data domain crate.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>>;
}

pub fn build_provider(cfg: &ProviderConfig) -> anyhow::Result<Box<dyn LlmProvider>> {
    let base_url = cfg.base_url.clone().unwrap_or_default();
    let provider: Box<dyn LlmProvider> = match cfg.kind.as_str() {
        "openai-compatible" | "openai_compatible" | "openai" | "deepseek" | "vllm"
        | "openrouter" => Box::new(OpenAiCompatibleProvider::new(base_url, cfg.api_key.clone())),
        "anthropic" | "anthropic-compatible" | "anthropic_compatible" => {
            Box::new(AnthropicProvider::new(base_url, cfg.api_key.clone()))
        }
        "gemini" | "google" => Box::new(GeminiProvider::new(base_url, cfg.api_key.clone())),
        other => anyhow::bail!("unsupported provider kind: {other}"),
    };
    Ok(provider)
}

pub fn model_from_config(model_config_json: &Option<String>, default_model: &str) -> String {
    model_config_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| {
            value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| default_model.to_string())
}

/// Whether an agent accepts provider-native image input.
///
/// Image attachments are a chat capability rather than an opt-in hidden behind
/// an advanced setting. Existing agents created before this setting was added
/// have no `vision` key, so absence means enabled; an explicit `false` still
/// lets an operator opt out for a text-only model.
pub fn vision_enabled(model_config_json: Option<&str>) -> bool {
    model_config_json
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.get("vision").and_then(Value::as_bool))
        .unwrap_or(true)
}

/// The reasoning depth an agent is configured with, if any.
///
/// `None` means the key is omitted from the provider request entirely, which
/// is what an agent left on the default thinking level wants. Every level the
/// agent form offers is a level here, so the deepest settings reach the
/// provider as themselves rather than as a shallower stand-in.
pub fn effort_from_config(model_config_json: Option<&str>) -> Option<ReasoningEffort> {
    let raw = model_config_json
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| {
            value
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::to_string)
        })?;
    ReasoningEffort::parse(&raw)
}

/// Accumulates the fragments of a single streamed tool call.
///
/// Provider deltas deliver a tool call's id, name and JSON arguments across
/// several chunks; this buffers them until the provider signals the call is
/// complete.
#[derive(Default)]
pub(crate) struct ToolAccum {
    pub id: String,
    pub name: String,
    pub args: String,
}

impl ToolAccum {
    /// Finalize the buffered fragments into a [`ChatDelta::ToolCall`], parsing
    /// the accumulated argument string as JSON (falling back to null when the
    /// fragments do not form valid JSON).
    pub fn finish(self) -> ChatDelta {
        let args = serde_json::from_str(&self.args).unwrap_or(Value::Null);
        ChatDelta::ToolCall(ToolCall {
            id: self.id,
            name: self.name,
            args,
            provider_metadata: None,
        })
    }
}

/// Drive a streaming HTTP response to completion, splitting it into lines and
/// feeding each line through `parse`. Any deltas the parser returns are
/// forwarded on `tx`. A terminating [`ChatDelta::Done`] is sent once the
/// upstream stream ends cleanly, regardless of whether the provider emitted an
/// explicit end-of-stream marker.
///
/// If the transport fails part-way through the body — a gateway idle timeout, a
/// dropped connection — the response is incomplete, so [`ChatDelta::Truncated`]
/// is sent instead of `Done`. Ending such a stream with `Done` would hand the
/// caller a partial answer that is indistinguishable from a complete one.
///
/// Bytes are buffered so that multi-byte UTF-8 sequences and SSE lines split
/// across network chunks are reassembled before parsing.
pub(crate) async fn pump<F>(resp: reqwest::Response, tx: Sender<ChatDelta>, mut parse: F)
where
    F: FnMut(&str) -> Vec<ChatDelta>,
{
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(error) => {
                let _ = tx.send(ChatDelta::Truncated(error.to_string())).await;
                return;
            }
        };
        buf.extend_from_slice(&chunk);

        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(['\r', '\n']);
            for delta in parse(line) {
                if tx.send(delta).await.is_err() {
                    return;
                }
            }
        }
    }

    // Flush any trailing line that arrived without a terminating newline.
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        for delta in parse(line.trim()) {
            if tx.send(delta).await.is_err() {
                return;
            }
        }
    }

    let _ = tx.send(ChatDelta::Done).await;
}

/// Extract the JSON payload of an SSE `data:` line.
///
/// Returns `None` for blank lines, comments, `event:`/`id:` fields and the
/// OpenAI `[DONE]` sentinel — none of which carry a JSON body to parse.
pub(crate) fn sse_data(line: &str) -> Option<&str> {
    let data = line.trim().strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::{model_context_config, model_reasoning_passback};

    #[test]
    fn model_context_config_uses_the_selected_model() {
        let raw = r#"[
            {"id":"small","context_window_tokens":32000,"context_output_reserve_ratio":0.2,"reasoning_passback":false},
            {"id":"large","context_window_tokens":128000,"context_output_reserve_ratio":0.3,"reasoning_passback":true}
        ]"#;

        assert_eq!(
            model_context_config(Some(raw), "large"),
            (Some(128000), Some(0.3))
        );
        assert_eq!(model_context_config(Some(raw), "missing"), (None, None));
        assert!(model_reasoning_passback(Some(raw), "large", false));
        assert!(!model_reasoning_passback(Some(raw), "small", true));
        assert!(!model_reasoning_passback(Some(raw), "missing", true));
        assert!(model_reasoning_passback(
            Some(r#"[{"id":"legacy"}]"#),
            "legacy",
            true
        ));
    }
}
