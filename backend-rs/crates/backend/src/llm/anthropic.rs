//! Anthropic Messages API provider.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{
    pump, sse_data, ChatDelta, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ToolAccum,
};
use ag_swarmer_domain::runtime::ChatContentPart;

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

fn split_system_and_messages(messages: &[ChatMessage]) -> (Option<Value>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut system_content_parts = Vec::new();
    let mut system_has_image = false;
    let mut out = Vec::new();

    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        if message.role == "system" {
            if !message.content.trim().is_empty() {
                system_parts.push(message.content.clone());
            }
            let has_image = message
                .parts
                .iter()
                .any(|part| matches!(part, ChatContentPart::Image { .. }));
            if has_image {
                system_has_image = true;
            }
            if has_image {
                system_content_parts.extend(anthropic_content_parts(&message.parts));
            } else if !message.content.trim().is_empty() {
                system_content_parts.push(json!({ "type": "text", "text": message.content }));
            }
            index += 1;
            continue;
        }
        if !message.tool_calls.is_empty() {
            let mut content = Vec::new();
            if !message.content.trim().is_empty() {
                content.push(json!({ "type": "text", "text": message.content }));
            }
            content.extend(message.tool_calls.iter().map(|call| {
                json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": call.args,
                })
            }));
            out.push(json!({ "role": "assistant", "content": content }));
            index += 1;
            continue;
        }
        if message.role == "tool" {
            let mut content = Vec::new();
            while index < messages.len() && messages[index].role == "tool" {
                let tool_message = &messages[index];
                content.push(json!({
                    "type": "tool_result",
                    "tool_use_id": tool_message.tool_call_id.as_deref().unwrap_or_default(),
                    "content": tool_message.content,
                }));
                index += 1;
            }
            out.push(json!({
                "role": "user",
                "content": content,
            }));
            continue;
        }
        if message.role == "user"
            && message
                .parts
                .iter()
                .any(|part| matches!(part, ChatContentPart::Image { .. }))
        {
            out.push(json!({
                "role": "user",
                "content": anthropic_content_parts(&message.parts),
            }));
            index += 1;
            continue;
        }
        out.push(json!({
            "role": message.role,
            "content": message.content,
        }));
        index += 1;
    }

    let system = if system_has_image {
        Some(Value::Array(system_content_parts))
    } else {
        (!system_parts.is_empty()).then(|| Value::String(system_parts.join("\n\n")))
    };
    (system, out)
}

fn anthropic_content_parts(parts: &[ChatContentPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|part| match part {
            ChatContentPart::Text { text } => json!({ "type": "text", "text": text }),
            ChatContentPart::Image {
                mime_type,
                data_base64,
            } => json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime_type,
                    "data": data_base64,
                },
            }),
        })
        .collect()
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
                    ..ContextUsage::default()
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
                    ..ContextUsage::default()
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
        let (system, messages) = split_system_and_messages(&request.messages);
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": DEFAULT_MAX_TOKENS,
            "stream": true,
        });
        if let Some(system) = system {
            body["system"] = system;
        }
        // Anthropic takes a token budget, not a level, and rejects one that is
        // not strictly below `max_tokens`. Clamp rather than trust the table:
        // if DEFAULT_MAX_TOKENS is ever lowered, the request must still be
        // valid instead of failing at the provider.
        if let Some(effort) = request.reasoning_effort {
            let ceiling = DEFAULT_MAX_TOKENS - 1;
            let budget = effort.thinking_budget_tokens().min(ceiling).max(1024);
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
        }
        if request.include_empty_tools || !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .into_iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.input_schema,
                        })
                    })
                    .collect(),
            );
        }

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
