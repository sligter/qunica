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
pub mod openai_responses;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use model_catalog::{discover_models, ModelCatalogError, ModelInfo, MODEL_CATALOG_TIMEOUT};
pub use openai_compatible::OpenAiCompatibleProvider;
pub use openai_responses::OpenAiResponsesProvider;

// Re-export the runtime data contract so integration tests (which link only
// against this crate) can name the shared types without depending on the domain
// crate directly. The domain crate holds only pure data types; the streaming
// provider behaviour below lives here in the backend.
pub use qunica_domain::runtime::{
    ChatDelta, ChatMessage, ChatRequest, ContextUsage, ReasoningEffort, ToolCall, ToolDefinition,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
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
    /// Whether the provider rejected the turn because the reasoning behind a
    /// tool call was not echoed back.
    ///
    /// DeepSeek's thinking mode states that, when `tools` is present, every
    /// previous `reasoning_content` must be passed back; a request with
    /// passback off answers with this 400. Matching the body is the only
    /// portable signal, and a false positive only costs one retry.
    pub fn requires_reasoning_passback(&self) -> bool {
        if self.status != 400 {
            return false;
        }
        let body = self.body.to_lowercase();
        body.contains("reasoning_content")
            && (body.contains("passed back") || body.contains("must be passed"))
    }

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
    pub headers: BTreeMap<String, String>,
    pub user_agent: Option<String>,
    pub default_model: String,
    pub reasoning_passback: bool,
    /// Whether the selected model accepts `temperature` / `top_p`.
    ///
    /// Resolved per model rather than per provider kind: an OpenAI reasoning
    /// model and a classic chat model share the same endpoint but disagree
    /// outright about sampling parameters.
    pub supports_temperature: bool,
    /// Whether the selected model accepts `reasoning_effort`.
    pub supports_reasoning_effort: bool,
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
    /// Explicit "this model accepts a reasoning-effort setting" opt-in.
    ///
    /// False is the stored default for historical providers, so it cannot mean
    /// "this model rejects the field": doing that would silently switch
    /// thinking off for every agent configured before the capability existed.
    /// The effective answer combines this opt-in with the model family — see
    /// [`model_capabilities`]. The provider form leaves the field alone; it is
    /// for an API client with a custom model name.
    #[serde(default)]
    pub supports_reasoning_effort: bool,
    /// Explicit sampling-parameter override, set through the API.
    ///
    /// `None` means infer from the model id. `Some(false)` is for a gateway
    /// that rejects `temperature` even though its model name looks ordinary.
    /// The provider form does not edit this, so inference stays authoritative.
    #[serde(default)]
    pub supports_temperature: Option<bool>,
    /// Whether prior reasoning is sent back during this model's tool loop.
    #[serde(default)]
    pub reasoning_passback: Option<bool>,
}

/// The per-model capabilities the provider request path has to honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub temperature: bool,
    pub reasoning_effort: bool,
}

fn stored_model_config(models_json: Option<&str>, model: &str) -> Option<ProviderModelConfig> {
    models_json
        .and_then(|raw| serde_json::from_str::<Vec<ProviderModelConfig>>(raw).ok())
        .and_then(|models| models.into_iter().find(|item| item.id == model))
}

/// The provider kinds whose wire format is the OpenAI chat dialect, where the
/// model name is the only signal for sampling/reasoning support.
fn is_openai_like_kind(kind: &str) -> bool {
    matches!(
        kind,
        "openai-compatible"
            | "openai_compatible"
            | "openai"
            | "openai-responses"
            | "deepseek"
            | "vllm"
            | "openrouter"
    )
}

/// The model id without a gateway's `vendor/` prefix or `:variant` suffix.
fn model_name_core(model: &str) -> String {
    let base = model.rsplit('/').next().unwrap_or(model);
    let base = base.split(':').next().unwrap_or(base);
    base.trim().to_ascii_lowercase()
}

/// OpenAI reasoning families reject `temperature`; their chat snapshots do not.
fn is_openai_reasoning_model(model: &str) -> bool {
    let name = model_name_core(model);
    if name.contains("-chat") {
        return false;
    }
    ["o1", "o3", "o4", "gpt-5", "gpt-6", "gpt-7", "codex"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// OpenAI chat models that are known **not** to reason. Sending
/// `reasoning_effort` to one is a provider error, so these are the only models
/// the request path withholds it from; everything unknown keeps the field, as
/// a custom gateway may well understand it.
fn is_known_non_reasoning_openai_model(model: &str) -> bool {
    let name = model_name_core(model);
    ["gpt-3", "gpt-4", "chatgpt"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || (name.starts_with("gpt-5") && name.contains("-chat"))
        || (name.starts_with("gpt-6") && name.contains("-chat"))
}

// Note: the model family is the source of truth and the stored flags are
// opt-in overrides. Every provider created before these capabilities existed
// stores `false`, so treating that as "model rejects it" would silently turn
// thinking off for existing agents.
/// Resolve the capabilities the request path must honour for `model`.
///
/// The stored flags are overrides, not the source of truth: a provider created
/// before either capability existed stores neither, and the model family is
/// what the provider will actually accept. For existing providers the practical
/// effect is "thinking still works, classic chat models stop receiving
/// `reasoning_effort`, and OpenAI reasoning models stop receiving
/// `temperature`".
pub fn model_capabilities(kind: &str, models_json: Option<&str>, model: &str) -> ModelCapabilities {
    let stored = stored_model_config(models_json, model);
    let openai_like = is_openai_like_kind(kind);
    let temperature = stored
        .as_ref()
        .and_then(|item| item.supports_temperature)
        .unwrap_or_else(|| !(openai_like && is_openai_reasoning_model(model)));
    let reasoning_effort = stored
        .as_ref()
        .is_some_and(|item| item.supports_reasoning_effort)
        || !openai_like
        || !is_known_non_reasoning_openai_model(model);
    ModelCapabilities {
        temperature,
        reasoning_effort,
    }
}

pub fn model_context_config(models_json: Option<&str>, model: &str) -> (Option<i64>, Option<f64>) {
    stored_model_config(models_json, model)
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

/// Read an agent's configured sampling temperature.
///
/// Out-of-range values are clamped rather than rejected: the form validates
/// them, and an imported or hand-edited config should not turn every turn into
/// a provider error.
pub fn temperature_from_config(model_config_json: Option<&str>) -> Option<f64> {
    sampling_from_config(model_config_json, "temperature", 0.0, 2.0)
}

/// Read an agent's configured `top_p`.
pub fn top_p_from_config(model_config_json: Option<&str>) -> Option<f64> {
    sampling_from_config(model_config_json, "top_p", 0.0, 1.0)
}

fn sampling_from_config(
    model_config_json: Option<&str>,
    key: &str,
    min: f64,
    max: f64,
) -> Option<f64> {
    let value = model_config_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get(key).and_then(serde_json::Value::as_f64))?;
    value.is_finite().then(|| value.clamp(min, max))
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
    let client = build_http_client(cfg, reqwest::Client::builder())?;
    let provider: Box<dyn LlmProvider> = match cfg.kind.as_str() {
        "openai-responses" => Box::new(OpenAiResponsesProvider::with_client(
            client,
            base_url,
            cfg.api_key.clone(),
        )),
        "openai-compatible" | "openai_compatible" | "openai" | "deepseek" | "vllm"
        | "openrouter" => Box::new(OpenAiCompatibleProvider::with_client(
            client,
            base_url,
            cfg.api_key.clone(),
        )),
        "anthropic" | "anthropic-compatible" | "anthropic_compatible" => Box::new(
            AnthropicProvider::with_client(client, base_url, cfg.api_key.clone()),
        ),
        "gemini" | "google" => Box::new(GeminiProvider::with_client(
            client,
            base_url,
            cfg.api_key.clone(),
        )),
        other => anyhow::bail!("unsupported provider kind: {other}"),
    };
    Ok(provider)
}

pub(crate) fn build_http_client(
    cfg: &ProviderConfig,
    mut builder: reqwest::ClientBuilder,
) -> anyhow::Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    for (name, value) in &cfg.headers {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes())?,
            HeaderValue::from_str(value)?,
        );
    }
    builder = builder.default_headers(headers);
    if let Some(user_agent) = cfg.user_agent.as_deref() {
        builder = builder.user_agent(user_agent);
    }
    Ok(builder.build()?)
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

pub fn provider_headers_from_json(raw: Option<&str>) -> BTreeMap<String, String> {
    raw.and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
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
pub(crate) async fn pump<F>(resp: reqwest::Response, tx: Sender<ChatDelta>, parse: F)
where
    F: FnMut(&str) -> Vec<ChatDelta>,
{
    pump_inner(resp, tx, parse, false).await;
}

/// Responses requires a terminal event; a clean HTTP EOF alone is not success.
pub(crate) async fn pump_terminal<F>(resp: reqwest::Response, tx: Sender<ChatDelta>, parse: F)
where
    F: FnMut(&str) -> Vec<ChatDelta>,
{
    pump_inner(resp, tx, parse, true).await;
}

async fn pump_inner<F>(
    resp: reqwest::Response,
    tx: Sender<ChatDelta>,
    mut parse: F,
    require_terminal: bool,
) where
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
                let terminal = matches!(delta, ChatDelta::Done | ChatDelta::Truncated(_));
                if tx.send(delta).await.is_err() || terminal {
                    return;
                }
            }
        }
    }

    // Flush any trailing line that arrived without a terminating newline.
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        for delta in parse(line.trim()) {
            let terminal = matches!(delta, ChatDelta::Done | ChatDelta::Truncated(_));
            if tx.send(delta).await.is_err() || terminal {
                return;
            }
        }
    }

    let end = if require_terminal {
        ChatDelta::Truncated("Responses stream ended without a terminal event".to_string())
    } else {
        ChatDelta::Done
    };
    let _ = tx.send(end).await;
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
    use super::{
        model_capabilities, model_context_config, model_reasoning_passback,
        temperature_from_config, top_p_from_config, ProviderHttpError,
    };

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

    #[test]
    fn model_capabilities_infer_the_model_family() {
        // OpenAI reasoning models reject sampling parameters but understand
        // the effort enum (their chat snapshots are the exception).
        for model in ["o3-mini", "o4-mini", "gpt-5.5", "openai/gpt-5:free"] {
            let capabilities = model_capabilities("openai-compatible", None, model);
            assert!(!capabilities.temperature, "{model}");
            assert!(capabilities.reasoning_effort, "{model}");
        }
        assert!(model_capabilities("openai-compatible", None, "gpt-5-chat-latest").temperature);
        assert!(model_capabilities("openai-compatible", None, "gpt-6-astra").reasoning_effort);

        // Classic chat models keep temperature and must not get the effort.
        let gpt4 = model_capabilities("openai-compatible", None, "gpt-4o-mini");
        assert!(gpt4.temperature);
        assert!(!gpt4.reasoning_effort);

        // DeepSeek accepts both (thinking mode ignores temperature rather than
        // rejecting it), and unknown custom names are left alone.
        let deepseek = model_capabilities("openai-compatible", None, "deepseek-reasoner");
        assert!(deepseek.temperature);
        assert!(deepseek.reasoning_effort);
        let custom = model_capabilities("openai-compatible", None, "my-reasoner");
        assert!(custom.temperature);
        assert!(custom.reasoning_effort);

        // Native thinking providers always take the field.
        let claude = model_capabilities("anthropic", None, "claude-sonnet-4-5");
        assert!(claude.temperature);
        assert!(claude.reasoning_effort);
    }

    #[test]
    fn model_capabilities_honour_explicit_overrides() {
        let raw = r#"[
            {"id":"gpt-4o","supports_temperature":false,"supports_reasoning_effort":true},
            {"id":"custom","supports_temperature":true,"supports_reasoning_effort":false}
        ]"#;

        let classic = model_capabilities("openai-compatible", Some(raw), "gpt-4o");
        assert!(!classic.temperature, "explicit temperature false wins");
        assert!(
            classic.reasoning_effort,
            "explicit effort true overrides the family inference"
        );

        let custom = model_capabilities("openai-compatible", Some(raw), "custom");
        assert!(custom.temperature);
        assert!(
            custom.reasoning_effort,
            "unknown names keep the field: historical configs store false by default"
        );
    }

    #[test]
    fn sampling_settings_are_read_as_f64_and_bounded() {
        let raw = r#"{"temperature":0.95,"top_p":0.8}"#;
        assert_eq!(temperature_from_config(Some(raw)), Some(0.95));
        assert_eq!(top_p_from_config(Some(raw)), Some(0.8));
        assert_eq!(
            temperature_from_config(Some(r#"{"temperature":9}"#)),
            Some(2.0)
        );
        assert_eq!(top_p_from_config(Some(r#"{"top_p":-1}"#)), Some(0.0));
        assert_eq!(
            temperature_from_config(Some(r#"{"temperature":"hot"}"#)),
            None
        );
        assert_eq!(temperature_from_config(Some("{}")), None);
    }

    #[test]
    fn deepseek_passback_rejection_is_recognised() {
        let error = ProviderHttpError {
            status: 400,
            body: "{\"error\":{\"message\":\"The `reasoning_content` in the thinking mode must be passed back to the API.\"}}".to_string(),
        };
        assert!(error.requires_reasoning_passback());
        let unrelated = ProviderHttpError {
            status: 400,
            body: "context length exceeded".to_string(),
        };
        assert!(!unrelated.requires_reasoning_passback());
        let wrong_status = ProviderHttpError {
            status: 500,
            body: "reasoning_content must be passed back".to_string(),
        };
        assert!(!wrong_status.requires_reasoning_passback());
    }
}
