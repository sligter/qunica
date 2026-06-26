//! OpenAI-compatible chat completions provider (OpenAI, DeepSeek, vLLM, etc.).

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{pump, sse_data, ChatDelta, ChatRequest, ContextUsage, LlmProvider, ToolAccum};

/// Streams chat completions from any endpoint that speaks the OpenAI
/// `/chat/completions` streaming protocol.
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl OpenAiCompatibleProvider {
    /// Create a provider targeting `base_url` (the API root, e.g.
    /// `https://api.openai.com/v1`). `/chat/completions` is appended per request.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }
}

/// Cross-chunk parser state: streamed tool calls keyed by their `index`.
#[derive(Default)]
struct State {
    tools: BTreeMap<i64, ToolAccum>,
}

/// Map a single OpenAI streaming chunk to zero or more [`ChatDelta`]s.
fn parse(line: &str, state: &mut State) -> Vec<ChatDelta> {
    let Some(data) = sse_data(line) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    if let Some(choice) = value["choices"].get(0) {
        let delta = &choice["delta"];

        if let Some(text) = delta["content"].as_str() {
            if !text.is_empty() {
                out.push(ChatDelta::Token(text.to_string()));
            }
        }

        // Reasoning models surface chain-of-thought under either key.
        if let Some(reasoning) = delta["reasoning_content"]
            .as_str()
            .or_else(|| delta["reasoning"].as_str())
        {
            if !reasoning.is_empty() {
                out.push(ChatDelta::Reasoning(reasoning.to_string()));
            }
        }

        if let Some(tool_calls) = delta["tool_calls"].as_array() {
            for tc in tool_calls {
                let index = tc["index"].as_i64().unwrap_or(0);
                let entry = state.tools.entry(index).or_default();
                if let Some(id) = tc["id"].as_str() {
                    if !id.is_empty() {
                        entry.id = id.to_string();
                    }
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    if !name.is_empty() {
                        entry.name = name.to_string();
                    }
                }
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    entry.args.push_str(args);
                }
            }
        }

        if choice["finish_reason"].as_str() == Some("tool_calls") {
            for (_, accum) in std::mem::take(&mut state.tools) {
                out.push(accum.finish());
            }
        }
    }

    let usage = &value["usage"];
    if !usage.is_null() {
        out.push(ChatDelta::Usage(ContextUsage {
            input_tokens: usage["prompt_tokens"].as_i64(),
            output_tokens: usage["completion_tokens"].as_i64(),
            total_tokens: usage["total_tokens"].as_i64(),
        }));
    }

    out
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": request.model,
            "messages": request.messages,
            "temperature": request.temperature,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
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
