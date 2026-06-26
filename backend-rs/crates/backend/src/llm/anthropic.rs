//! Anthropic Messages API provider.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{pump, sse_data, ChatDelta, ChatRequest, ContextUsage, LlmProvider, ToolAccum};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: i64 = 4096;

/// Streams responses from the Anthropic `/v1/messages` endpoint.
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicProvider {
    /// Create a provider targeting `base_url` (the API root, e.g.
    /// `https://api.anthropic.com`). `/v1/messages` is appended per request.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }
}

/// Cross-event parser state: tool-use blocks keyed by their content-block index.
#[derive(Default)]
struct State {
    tools: BTreeMap<i64, ToolAccum>,
}

/// Map a single Anthropic stream event to zero or more [`ChatDelta`]s.
fn parse(line: &str, state: &mut State) -> Vec<ChatDelta> {
    let Some(data) = sse_data(line) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    match value["type"].as_str().unwrap_or_default() {
        "message_start" => {
            if let Some(input) = value["message"]["usage"]["input_tokens"].as_i64() {
                out.push(ChatDelta::Usage(ContextUsage {
                    input_tokens: Some(input),
                    output_tokens: None,
                    total_tokens: None,
                }));
            }
        }
        "content_block_start" => {
            let block = &value["content_block"];
            if block["type"].as_str() == Some("tool_use") {
                let index = value["index"].as_i64().unwrap_or(0);
                let entry = state.tools.entry(index).or_default();
                entry.id = block["id"].as_str().unwrap_or_default().to_string();
                entry.name = block["name"].as_str().unwrap_or_default().to_string();
            }
        }
        "content_block_delta" => {
            let delta = &value["delta"];
            match delta["type"].as_str().unwrap_or_default() {
                "text_delta" => {
                    if let Some(text) = delta["text"].as_str() {
                        out.push(ChatDelta::Token(text.to_string()));
                    }
                }
                "thinking_delta" => {
                    if let Some(thinking) = delta["thinking"].as_str() {
                        out.push(ChatDelta::Reasoning(thinking.to_string()));
                    }
                }
                "input_json_delta" => {
                    let index = value["index"].as_i64().unwrap_or(0);
                    if let Some(partial) = delta["partial_json"].as_str() {
                        state.tools.entry(index).or_default().args.push_str(partial);
                    }
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            let index = value["index"].as_i64().unwrap_or(0);
            if let Some(accum) = state.tools.remove(&index) {
                out.push(accum.finish());
            }
        }
        "message_delta" => {
            if let Some(output) = value["usage"]["output_tokens"].as_i64() {
                out.push(ChatDelta::Usage(ContextUsage {
                    input_tokens: None,
                    output_tokens: Some(output),
                    total_tokens: None,
                }));
            }
        }
        _ => {}
    }

    out
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": request.model,
            "messages": request.messages,
            "temperature": request.temperature,
            "max_tokens": DEFAULT_MAX_TOKENS,
            "stream": true,
        });

        let resp = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut state = State::default();
            pump(resp, tx, move |line| parse(line, &mut state)).await;
        });
        Ok(rx)
    }
}
