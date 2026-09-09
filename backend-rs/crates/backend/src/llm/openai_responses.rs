//! OpenAI Responses API, using stateless history and streaming output events.

use std::collections::BTreeMap;

use async_trait::async_trait;
use qunica_domain::runtime::ChatContentPart;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{ChatDelta, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ToolCall};

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl OpenAiResponsesProvider {
    /// Accept an API root or a full `/responses` endpoint; empty uses OpenAI.
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

    fn endpoint(&self) -> anyhow::Result<reqwest::Url> {
        let base = self.base_url.trim();
        let mut url = reqwest::Url::parse(if base.is_empty() {
            DEFAULT_BASE_URL
        } else {
            base
        })?;
        let path = url.path().trim_end_matches('/');
        let path = if path.ends_with("/responses") {
            path.to_string()
        } else {
            format!("{path}/responses")
        };
        url.set_path(&path);
        url.set_fragment(None);
        Ok(url)
    }
}

fn to_input(messages: &[ChatMessage]) -> Vec<Value> {
    let mut input = Vec::new();
    for message in messages {
        if message.role == "tool" {
            input.push(json!({
                "type": "function_call_output",
                "call_id": message.tool_call_id.as_deref().unwrap_or_default(),
                "output": message.content,
            }));
            continue;
        }
        // Encrypted reasoning belongs to the tool turn that produced it. Keep it
        // on the first call's existing metadata for the active tool-loop replay.
        for call in &message.tool_calls {
            if let Some(items) = call
                .provider_metadata
                .as_ref()
                .and_then(|meta| meta["openai_responses_reasoning"].as_array())
            {
                input.extend(
                    items
                        .iter()
                        .filter(|item| item["type"] == "reasoning")
                        .cloned(),
                );
            }
        }
        if !message.content.is_empty() || !message.parts.is_empty() || message.tool_calls.is_empty()
        {
            let content = if message.role == "user" && !message.parts.is_empty() {
                Value::Array(
                    message
                        .parts
                        .iter()
                        .map(|part| match part {
                            ChatContentPart::Text { text } => {
                                json!({"type": "input_text", "text": text})
                            }
                            ChatContentPart::Image {
                                mime_type,
                                data_base64,
                            } => json!({
                                "type": "input_image",
                                "image_url": format!("data:{mime_type};base64,{data_base64}"),
                            }),
                        })
                        .collect(),
                )
            } else {
                json!(message.content)
            };
            input.push(json!({"role": message.role, "content": content}));
        }
        for call in &message.tool_calls {
            input.push(json!({
                "type": "function_call", "call_id": call.id,
                "name": call.name, "arguments": call.args.to_string(),
            }));
        }
    }
    input
}

fn request_body(request: &ChatRequest) -> Value {
    let mut body = json!({
        "model": request.model,
        "input": to_input(&request.messages),
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
    });
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(effort) = request.reasoning_effort {
        body["reasoning"] = json!({"effort": effort.as_str(), "summary": "auto"});
    }
    if request.include_empty_tools || !request.tools.is_empty() {
        body["tools"] = json!(request
            .tools
            .iter()
            .map(|tool| json!({
                "type": "function", "name": tool.name,
                "description": tool.description, "parameters": tool.input_schema,
                // Preserve optional fields in the application's existing schemas.
                "strict": false,
            }))
            .collect::<Vec<_>>());
    }
    body
}

#[derive(Default)]
struct State {
    // Done items contain complete arguments, including when deltas interleave.
    // Calls are released only after the entire response completes successfully.
    items: BTreeMap<i64, Value>,
}

fn usage(response: &Value) -> Option<ChatDelta> {
    let usage = response.get("usage").filter(|value| value.is_object())?;
    Some(ChatDelta::Usage(ContextUsage {
        input_tokens: usage["input_tokens"].as_i64(),
        cached_input_tokens: usage["input_tokens_details"]["cached_tokens"].as_i64(),
        output_tokens: usage["output_tokens"].as_i64(),
        total_tokens: usage["total_tokens"].as_i64(),
        ..ContextUsage::default()
    }))
}

fn parse(line: &str, state: &mut State) -> Vec<ChatDelta> {
    let Some(data) = super::sse_data(line) else {
        return Vec::new();
    };
    let value: Value = match serde_json::from_str(data) {
        Ok(value) => value,
        Err(_) => return vec![ChatDelta::Truncated("Invalid Responses stream JSON".into())],
    };
    match value["type"].as_str().unwrap_or_default() {
        "response.output_text.delta" | "response.refusal.delta" => value["delta"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(|text| vec![ChatDelta::Token(text.into())])
            .unwrap_or_default(),
        "response.reasoning_summary_text.delta" => value["delta"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(|text| vec![ChatDelta::Reasoning(text.into())])
            .unwrap_or_default(),
        "response.output_item.done" => {
            if let Some(index) = value["output_index"].as_i64() {
                state.items.insert(index, value["item"].clone());
            }
            Vec::new()
        }
        "response.completed" => {
            let items = value["response"]["output"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| std::mem::take(&mut state.items).into_values().collect());
            let reasoning: Vec<_> = items
                .iter()
                .filter(|item| item["type"] == "reasoning" && item["encrypted_content"].is_string())
                .cloned()
                .collect();
            let mut calls = Vec::new();
            for item in items.iter().filter(|item| item["type"] == "function_call") {
                let Some(id) = item["call_id"].as_str().filter(|id| !id.is_empty()) else {
                    return vec![ChatDelta::Truncated(
                        "Responses tool call is missing call_id".into(),
                    )];
                };
                let Some(name) = item["name"].as_str().filter(|name| !name.is_empty()) else {
                    return vec![ChatDelta::Truncated(
                        "Responses tool call is missing name".into(),
                    )];
                };
                let args = item["arguments"]
                    .as_str()
                    .and_then(|args| serde_json::from_str::<Value>(args).ok());
                let Some(args) = args.filter(Value::is_object) else {
                    return vec![ChatDelta::Truncated(
                        "Invalid Responses tool call arguments".into(),
                    )];
                };
                calls.push(ChatDelta::ToolCall(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    args,
                    provider_metadata: if calls.is_empty() && !reasoning.is_empty() {
                        Some(json!({"openai_responses_reasoning": reasoning}))
                    } else {
                        None
                    },
                }));
            }
            calls.extend(usage(&value["response"]));
            calls.push(ChatDelta::Done);
            calls
        }
        "response.failed" | "response.incomplete" | "error" => {
            let reason = value["response"]["error"]["message"]
                .as_str()
                .or_else(|| value["response"]["incomplete_details"]["reason"].as_str())
                .or_else(|| value["message"].as_str())
                .or_else(|| value["error"]["message"].as_str())
                .unwrap_or("Responses request failed");
            let mut out: Vec<_> = usage(&value["response"]).into_iter().collect();
            out.push(ChatDelta::Truncated(reason.into()));
            out
        }
        _ => Vec::new(),
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let resp = self
            .client
            .post(self.endpoint()?)
            .bearer_auth(&self.api_key)
            .json(&request_body(&request))
            .send()
            .await?;
        let resp = super::ensure_success(resp).await?;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut state = State::default();
            super::pump_terminal(resp, tx, |line| parse(line, &mut state)).await;
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_accept_roots_and_full_paths_without_losing_queries() {
        for (base, expected) in [
            ("", "https://api.openai.com/v1/responses"),
            (
                " https://gateway.test/v1/ ",
                "https://gateway.test/v1/responses",
            ),
            (
                "https://gateway.test/v1/responses/?tenant=a#fragment",
                "https://gateway.test/v1/responses?tenant=a",
            ),
        ] {
            assert_eq!(
                OpenAiResponsesProvider::new(base, "key")
                    .endpoint()
                    .unwrap()
                    .as_str(),
                expected
            );
        }
    }

    #[test]
    fn completed_output_deduplicates_done_items_and_refusal_is_visible() {
        let mut state = State::default();
        let item =
            json!({"type":"function_call","call_id":"call_1","name":"Read","arguments":"{}"});
        assert!(parse(
            &format!(
                "data: {}",
                json!({"type":"response.output_item.done","output_index":0,"item":item})
            ),
            &mut state
        )
        .is_empty());
        let deltas = parse(
            &format!(
                "data: {}",
                json!({"type":"response.completed","response":{"output":[item]}})
            ),
            &mut state,
        );
        assert_eq!(deltas.len(), 2);
        assert!(matches!(&deltas[0], ChatDelta::ToolCall(call) if call.id == "call_1"));
        assert!(matches!(deltas[1], ChatDelta::Done));
        let deltas = parse(
            r#"data: {"type":"response.refusal.delta","delta":"Cannot help"}"#,
            &mut State::default(),
        );
        assert!(matches!(&deltas[0], ChatDelta::Token(text) if text == "Cannot help"));
    }
}
