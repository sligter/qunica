//! OpenAI-compatible chat completions provider (OpenAI, DeepSeek, vLLM, etc.).

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{
    pump, sse_data, ChatDelta, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ToolAccum,
};
use ag_swarmer_domain::runtime::ChatContentPart;

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

fn to_messages(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| {
            if !message.tool_calls.is_empty() {
                return json!({
                    "role": "assistant",
                    "content": message.content,
                    "tool_calls": message.tool_calls.iter().map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.args.to_string(),
                            },
                        })
                    }).collect::<Vec<_>>(),
                });
            }
            if message.role == "tool" {
                return json!({
                    "role": "tool",
                    "tool_call_id": message.tool_call_id.as_deref().unwrap_or_default(),
                    "name": message.tool_name.as_deref().unwrap_or_default(),
                    "content": message.content,
                });
            }
            if matches!(message.role.as_str(), "user" | "system")
                && message
                    .parts
                    .iter()
                    .any(|part| matches!(part, ChatContentPart::Image { .. }))
            {
                return json!({
                    "role": message.role,
                    "content": openai_content_parts(&message.parts),
                });
            }
            json!({
                "role": message.role,
                "content": message.content,
            })
        })
        .collect()
}

fn openai_content_parts(parts: &[ChatContentPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|part| match part {
            ChatContentPart::Text { text } => json!({ "type": "text", "text": text }),
            ChatContentPart::Image {
                mime_type,
                data_base64,
            } => json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{mime_type};base64,{data_base64}") },
            }),
        })
        .collect()
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

        // `tool_calls` is the documented finish reason for a tool-calling turn,
        // but several gateways close the same turn with `stop`. Waiting for the
        // documented value dropped the buffered calls on those providers, so the
        // agent's round ended with neither a tool result nor visible text — a
        // turn that looks like the model simply said nothing. Truncated turns
        // (`length`, `content_filter`) are still dropped: their argument JSON is
        // incomplete, so the call must not be executed.
        if matches!(
            choice["finish_reason"].as_str(),
            Some("tool_calls") | Some("stop")
        ) {
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
            ..ContextUsage::default()
        }));
    }

    out
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut body = json!({
            "model": request.model,
            "messages": to_messages(&request.messages),
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        // Absent rather than null: strict gateways may reject optional keys outright.
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(effort) = request.reasoning_effort {
            body["reasoning_effort"] = json!(effort.as_str());
        }
        if request.include_empty_tools || !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .into_iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                            },
                        })
                    })
                    .collect(),
            );
        }

        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let resp = super::ensure_success(resp).await?;

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut state = State::default();
            pump(resp, tx, move |line| parse(line, &mut state)).await;
        });
        Ok(rx)
    }
}
