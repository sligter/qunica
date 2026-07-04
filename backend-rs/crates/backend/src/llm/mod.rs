//! LLM provider clients.
//!
//! Each provider implements [`LlmProvider`] by issuing a streaming HTTP request
//! and translating provider-specific server-sent events into the neutral
//! [`ChatDelta`] vocabulary defined by the domain runtime contract. The clients
//! here do parsing and mapping only — request orchestration, tool execution and
//! group runtime live in later tasks.

pub mod anthropic;
pub mod gemini;
pub mod openai_compatible;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use openai_compatible::OpenAiCompatibleProvider;

// Re-export the runtime data contract so integration tests (which link only
// against this crate) can name the shared types without depending on the domain
// crate directly. The domain crate holds only pure data types; the streaming
// provider behaviour below lives here in the backend.
pub use ag_swarmer_domain::runtime::{
    ChatDelta, ChatMessage, ChatRequest, ContextUsage, ToolCall, ToolDefinition,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc::{Receiver, Sender};

/// Resolved LLM provider connection settings.
pub struct ProviderConfig {
    pub kind: String,
    pub base_url: Option<String>,
    pub api_key: String,
    pub default_model: String,
    pub reasoning_passback: bool,
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
/// forwarded on `tx`. A terminating [`ChatDelta::Done`] is always sent once the
/// upstream stream ends, regardless of whether the provider emitted an explicit
/// end-of-stream marker.
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
            Err(_) => break,
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
