//! Google Gemini `generateContent` streaming provider.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver};

use super::{
    pump, sse_data, ChatDelta, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ToolCall,
};

/// Streams responses from the Gemini `:streamGenerateContent` endpoint.
pub struct GeminiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl GeminiProvider {
    /// Create a provider targeting `base_url` (the API root, e.g.
    /// `https://generativelanguage.googleapis.com`).
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }
}

/// Translate neutral chat messages into Gemini `contents` entries.
fn to_contents(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let role = if m.role == "assistant" {
                "model"
            } else {
                "user"
            };
            json!({ "role": role, "parts": [{ "text": m.content }] })
        })
        .collect()
}

/// Map a single Gemini stream chunk to zero or more [`ChatDelta`]s.
fn parse(line: &str) -> Vec<ChatDelta> {
    let Some(data) = sse_data(line) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    if let Some(candidates) = value["candidates"].as_array() {
        for candidate in candidates {
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts {
                    if let Some(text) = part["text"].as_str() {
                        if !text.is_empty() {
                            out.push(ChatDelta::Token(text.to_string()));
                        }
                    }
                    let function_call = &part["functionCall"];
                    if !function_call.is_null() {
                        let name = function_call["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        out.push(ChatDelta::ToolCall(ToolCall {
                            // Gemini does not assign tool-call ids; the name is
                            // the stable identifier for the call.
                            id: name.clone(),
                            name,
                            args: function_call["args"].clone(),
                        }));
                    }
                }
            }
        }
    }

    let usage = &value["usageMetadata"];
    if !usage.is_null() {
        out.push(ChatDelta::Usage(ContextUsage {
            input_tokens: usage["promptTokenCount"].as_i64(),
            output_tokens: usage["candidatesTokenCount"].as_i64(),
            total_tokens: usage["totalTokenCount"].as_i64(),
        }));
    }

    out
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url.trim_end_matches('/'),
            request.model,
            self.api_key,
        );
        let mut generation_config = serde_json::Map::new();
        if let Some(temp) = request.temperature {
            generation_config.insert("temperature".to_string(), json!(temp));
        }
        let mut body = json!({
            "contents": to_contents(&request.messages),
            "generationConfig": Value::Object(generation_config),
        });
        if !request.tools.is_empty() {
            body["tools"] = json!([{
                "functionDeclarations": request
                    .tools
                    .into_iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema,
                        })
                    })
                    .collect::<Vec<_>>()
            }]);
        }

        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            pump(resp, tx, parse).await;
        });
        Ok(rx)
    }
}
