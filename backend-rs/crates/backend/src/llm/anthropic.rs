//! Anthropic Messages API provider.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{
    pump, sse_data, ChatDelta, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ToolAccum,
};
use qunica_domain::runtime::ChatContentPart;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: i64 = 4096;
/// Room left for the answer on top of a thinking budget.
///
/// `max_tokens` covers thinking *and* the reply, and Anthropic rejects a
/// request whose budget is not strictly below it, so a deeper thinking level
/// has to raise the ceiling with it — otherwise the model spends its whole
/// allowance thinking and has nothing left to say.
const THINKING_RESPONSE_RESERVE: i64 = DEFAULT_MAX_TOKENS;
/// The shallowest thinking Anthropic accepts; anything less is an error.
const MIN_THINKING_BUDGET: i64 = 1024;

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
        Self::with_client(reqwest::Client::new(), base_url, api_key)
    }

    pub(crate) fn with_client(
        client: reqwest::Client,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }
}

/// Split the neutral message list into Anthropic's `system` field and its
/// `messages` array.
///
/// `thinking_enabled` decides whether an assistant turn's own thinking travels
/// back with it. Anthropic requires that: with thinking on, the assistant
/// message that made the tool calls must lead with the signed thinking block
/// that produced them, and a request whose tool results follow a plain text
/// block is rejected outright. It is equally an error to send a thinking block
/// when thinking is off, so the flag gates both directions.
fn split_system_and_messages(
    messages: &[ChatMessage],
    thinking_enabled: bool,
) -> (Option<Value>, Vec<Value>) {
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
            if thinking_enabled {
                content.extend(thinking_block(message));
            }
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

/// The signed thinking that produced this assistant message, as the content
/// block Anthropic wants back.
///
/// Empty unless the message carries both the thinking and the signature over
/// it. Reasoning replayed from stored history arrives without a signature, and
/// an unsigned or re-signed block fails verification — so the choice is between
/// sending nothing and sending something the provider rejects. Only the turn
/// currently in flight has a signature, which is also the only turn Anthropic
/// requires one for.
fn thinking_block(message: &ChatMessage) -> Option<Value> {
    let thinking = message
        .reasoning_content
        .as_deref()
        .filter(|text| !text.trim().is_empty())?;
    let signature = message.reasoning_signature.as_deref()?;
    Some(json!({
        "type": "thinking",
        "thinking": thinking,
        "signature": signature,
    }))
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
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
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
                let usage = &value["message"]["usage"];
                let cached = usage["cache_read_input_tokens"].as_i64().unwrap_or(0);
                let created = usage["cache_creation_input_tokens"].as_i64().unwrap_or(0);
                let input = input.saturating_add(cached).saturating_add(created);
                state.input_tokens = Some(input);
                state.cached_input_tokens = Some(cached);
                out.push(ChatDelta::Usage(ContextUsage {
                    input_tokens: Some(input),
                    cached_input_tokens: Some(cached),
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
                // Closes a thinking block: the provider's signature over the
                // thinking that just streamed. Carried out of the stream so the
                // block can travel back with the tool calls it produced.
                "signature_delta" => {
                    if let Some(signature) = delta["signature"].as_str() {
                        out.push(ChatDelta::ReasoningSignature(signature.to_string()));
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
                let total = state.input_tokens.map(|input| input.saturating_add(output));
                out.push(ChatDelta::Usage(ContextUsage {
                    input_tokens: state.input_tokens,
                    cached_input_tokens: state.cached_input_tokens,
                    output_tokens: Some(output),
                    total_tokens: total,
                    ..ContextUsage::default()
                }));
            }
        }
        // A fault raised mid-stream — an overload, an expired credential — after
        // the response already began. The HTTP status was 200, so nothing else
        // marks this as a failure: without it the round ends on `Done` and a
        // dropped answer reads as a model that chose to say nothing.
        "error" => {
            let reason = value["error"]["message"]
                .as_str()
                .or_else(|| value["error"]["type"].as_str())
                .unwrap_or("provider error");
            out.push(ChatDelta::Truncated(reason.to_string()));
        }
        _ => {}
    }

    out
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let (system, messages) =
            split_system_and_messages(&request.messages, request.reasoning_effort.is_some());
        // Anthropic takes a token budget, not a level. `max_tokens` covers the
        // thinking as well as the reply and must stay above the budget, so a
        // deeper level raises both together rather than being clamped down to
        // fit a fixed ceiling.
        let thinking = request
            .reasoning_effort
            .map(|effort| effort.thinking_budget_tokens().max(MIN_THINKING_BUDGET));
        let max_tokens = thinking.map_or(DEFAULT_MAX_TOKENS, |budget| {
            budget.saturating_add(THINKING_RESPONSE_RESERVE)
        });
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "stream": true,
        });
        // Extended thinking fixes sampling: Anthropic rejects any temperature
        // but 1 while it is on. Absent rather than null in every other case —
        // the API validates the field's type, so a null is a rejected request,
        // not an omitted setting.
        if let Some(temperature) = request.temperature.filter(|_| thinking.is_none()) {
            body["temperature"] = json!(temperature);
        }
        if let Some(system) = system {
            body["system"] = system;
        }
        if let Some(budget) = thinking {
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

#[cfg(test)]
mod tests {
    use super::{parse, ChatDelta, State};

    #[test]
    fn final_usage_includes_anthropic_cache_tokens() {
        let mut state = State::default();
        parse(
            r#"data: {"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":25,"cache_creation_input_tokens":5}}}"#,
            &mut state,
        );
        let deltas = parse(
            r#"data: {"type":"message_delta","usage":{"output_tokens":2}}"#,
            &mut state,
        );

        let ChatDelta::Usage(usage) = &deltas[0] else {
            panic!("expected usage delta");
        };
        assert_eq!(usage.input_tokens, Some(40));
        assert_eq!(usage.cached_input_tokens, Some(25));
        assert_eq!(usage.output_tokens, Some(2));
        assert_eq!(usage.total_tokens, Some(42));
    }
}
