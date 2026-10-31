//! [`LlmProvider`] backed by Rig (feature `rig-engine`).
//!
//! Rig sends the request and decodes the stream; Qunica keeps the request
//! settings, the message history, the transport and the error classification.
//! Runtime routing does not select this provider yet. See RIG_MIGRATION.md.

mod adapter;

pub use adapter::RigMessageAdapter;

use async_trait::async_trait;
use futures_util::StreamExt;
use rig_core::completion::{AssistantContent, CompletionRequest, CompletionResponse, FinishReason};
use rig_core::http_client::{Bytes, DynHttpClient, Exchange, HeaderMap, HttpMiddleware};
use rig_core::message::{ToolCall as RigToolCall, ToolName};
use rig_core::operation::Completion;
use rig_core::providers::openai::wire::{Dialect, Quirks};
use rig_core::providers::openai::{OpenAI, OpenAIConfig};
use rig_core::streaming::{Item, StreamEvent};
use rig_core::wasm_compat::WasmBoxedFuture;
use rig_core::{DynModel, ProviderError};
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver, Sender};

use super::{
    ChatDelta, ChatRequest, ContextUsage, LlmProvider, ProviderConfig, ProviderHttpError,
    MAX_ERROR_BODY_CHARS,
};

/// A provider whose requests go through Rig.
pub struct RigProvider {
    client: OpenAI,
}

impl RigProvider {
    /// An OpenAI-compatible `/chat/completions` endpoint at `base_url`, the
    /// counterpart of [`super::OpenAiCompatibleProvider::new`].
    pub fn openai_compatible(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> anyhow::Result<Self> {
        Self::with_http(
            base_url.into(),
            api_key.into(),
            transport(&Default::default(), None)?,
        )
    }

    fn with_http(base_url: String, api_key: String, http: DynHttpClient) -> anyhow::Result<Self> {
        let client = OpenAIConfig::with_key(&gateway(), api_key)
            .with_base_url(base_url)
            .connect(http);
        Ok(Self { client })
    }

    fn model(&self, name: &str) -> DynModel<Completion> {
        self.client.chat(name).into()
    }
}

/// The Rig provider for `cfg`, for the kinds Rig serves so far.
pub fn build_rig_provider(cfg: &ProviderConfig) -> anyhow::Result<Box<dyn LlmProvider>> {
    let http = transport(&cfg.headers, cfg.user_agent.as_deref())?;
    let base_url = cfg.base_url.clone().unwrap_or_default();
    match cfg.kind.as_str() {
        "openai-compatible" | "openai_compatible" | "openai" | "deepseek" | "vllm"
        | "openrouter" => Ok(Box::new(RigProvider::with_http(
            base_url,
            cfg.api_key.clone(),
            http,
        )?)),
        other => anyhow::bail!("the Rig engine does not serve provider kind {other} yet"),
    }
}

/// Qunica's own reqwest 0.13 client, handed to Rig.
///
/// Note: configured headers are reqwest defaults, which reqwest adds only
/// where the request has no such header. Rig's credential header is set per
/// request, so a configured `Authorization` never replaces the key, exactly
/// as on the native path.
fn transport(
    headers: &std::collections::BTreeMap<String, String>,
    user_agent: Option<&str>,
) -> anyhow::Result<DynHttpClient> {
    use reqwest_013::header::{HeaderMap, HeaderName, HeaderValue};
    let mut defaults = HeaderMap::new();
    for (name, value) in headers {
        defaults.insert(
            HeaderName::from_bytes(name.as_bytes())?,
            HeaderValue::from_str(value)?,
        );
    }
    let mut builder = reqwest_013::Client::builder().default_headers(defaults);
    if let Some(user_agent) = user_agent {
        builder = builder.user_agent(user_agent);
    }
    Ok(
        DynHttpClient::new(rig_reqwest::ReqwestClient::from(builder.build()?))
            .with_middleware(OmitBlankReasoning),
    )
}

/// Note: once a dialect names a reasoning field, Rig's chat encoder writes it
/// on every assistant message, empty when there is none. Strict gateways
/// reject the empty field and the native path never sends one, so it is
/// removed on the way out.
struct OmitBlankReasoning;

impl HttpMiddleware for OmitBlankReasoning {
    fn before_request_body<'a>(
        &'a self,
        _exchange: Exchange<'a>,
        _headers: &'a HeaderMap,
        body: Bytes,
    ) -> WasmBoxedFuture<'a, rig_core::http_client::Result<Bytes>> {
        Box::pin(async move { Ok(omit_blank_reasoning(body)) })
    }
}

fn omit_blank_reasoning(body: Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return body;
    };
    let mut changed = false;
    for message in messages.iter_mut().filter_map(Value::as_object_mut) {
        let blank = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(|text| text.trim().is_empty());
        if blank {
            message.remove("reasoning_content");
            changed = true;
        }
    }
    if !changed {
        return body;
    }
    serde_json::to_vec(&value).map(Bytes::from).unwrap_or(body)
}

/// The OpenAI-compatible wire as the native provider speaks it.
///
/// Note: one neutral dialect for every compatible host rather than Rig's
/// vendor dialects. Those rewrite bodies per vendor (DeepSeek's thinking
/// switch, OpenAI's token cap), which would change requests that work today.
fn gateway() -> Dialect {
    let mut quirks = Quirks::openai().done_without_finish_reason();
    quirks.reasoning_field = Some("reasoning_content");
    let mut dialect = Dialect::gateway("openai-compatible", "", "");
    dialect.quirks = quirks;
    dialect
}

fn completion_request(request: &ChatRequest) -> anyhow::Result<CompletionRequest> {
    let mut history = RigMessageAdapter::to_rig(&request.messages)?;
    // Note: the per-model passback switch decides whether past reasoning is
    // sent, as on the native path; Rig would replay a same-origin turn's
    // reasoning unconditionally.
    for message in &mut history {
        if let rig_core::completion::Message::Assistant(turn) = message {
            turn.content.retain(|block| match block {
                AssistantContent::Reasoning(reasoning) => {
                    request.reasoning_passback && !reasoning.text.trim().is_empty()
                }
                _ => true,
            });
        }
    }
    let mut rig = CompletionRequest::from(history).temperature(request.temperature);
    for tool in &request.tools {
        rig = rig.tool(rig_core::completion::ToolDefinition::new(
            ToolName::new(tool.name.as_str())
                .map_err(|_| anyhow::anyhow!("tool definition without a name"))?,
            tool.description.clone(),
            tool.input_schema.clone(),
        ));
    }
    let mut extra = serde_json::Map::new();
    // Rig maps `top_p` only for hosts it knows; on a gateway it refuses it.
    if let Some(top_p) = request.top_p {
        extra.insert("top_p".into(), json!(top_p));
    }
    // The level as chosen, as the native path sends it: Rig's mapping would
    // round levels the catalog does not list for an unknown model.
    if let Some(effort) = request.reasoning_effort {
        extra.insert("reasoning_effort".into(), json!(effort.as_str()));
    }
    if request.include_empty_tools && request.tools.is_empty() {
        extra.insert("tools".into(), json!([]));
    }
    if !extra.is_empty() {
        rig = rig.additional_params(Value::Object(extra));
    }
    rig.foreign_reasoning = request.reasoning_passback;
    Ok(rig)
}

#[async_trait]
impl LlmProvider for RigProvider {
    async fn stream(&self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let rig_request = completion_request(&request)?;
        let mut stream = self
            .model(&request.model)
            .stream(rig_request)
            .map_err(into_anyhow)?;
        // Note: Rig sends nothing until polled. Reading the first item here
        // makes an HTTP failure an `Err` from `stream`, where the runtime
        // classifies it (context overflow, passback retry, transient).
        let first = match stream.next().await {
            Some(Err(error)) => return Err(into_anyhow(error)),
            first => first,
        };
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            relay(stream, first, tx).await;
        });
        Ok(rx)
    }
}

type RigStream = rig_core::streaming::Streamed<Completion>;
type RigItem = Option<Result<Item<StreamEvent>, ProviderError>>;

async fn relay(mut stream: RigStream, mut item: RigItem, tx: Sender<ChatDelta>) {
    let mut calls: Vec<RigToolCall> = Vec::new();
    loop {
        let delta = match item {
            None => break,
            Some(Ok(Item::Event(StreamEvent::Text { text, .. }))) if !text.is_empty() => {
                Some(ChatDelta::Token(text))
            }
            Some(Ok(Item::Event(StreamEvent::Reasoning { text, .. }))) if !text.is_empty() => {
                Some(ChatDelta::Reasoning(text))
            }
            Some(Ok(Item::Event(StreamEvent::End {
                content: AssistantContent::ToolCall(call),
                ..
            }))) => {
                calls.push(call);
                None
            }
            Some(Ok(_)) => None,
            Some(Err(error)) => {
                let _ = tx.send(ChatDelta::Truncated(stream_failure(&error))).await;
                return;
            }
        };
        if let Some(delta) = delta {
            if tx.send(delta).await.is_err() {
                return;
            }
        }
        item = stream.next().await;
    }
    let response = match stream.finish().await {
        Ok(response) => response,
        Err(error) => {
            let _ = tx.send(ChatDelta::Truncated(stream_failure(&error))).await;
            return;
        }
    };
    for delta in closing_deltas(&response, calls) {
        if tx.send(delta).await.is_err() {
            return;
        }
    }
}

/// Tool calls, usage and the stored turn, then `Done`.
fn closing_deltas(response: &CompletionResponse, calls: Vec<RigToolCall>) -> Vec<ChatDelta> {
    let mut out = Vec::new();
    // As on the native path: only a turn that finished cleanly may run its
    // calls. A cut-off turn's argument JSON is incomplete.
    if matches!(
        response.finish_reason(),
        Some(FinishReason::Stop | FinishReason::ToolCalls)
    ) {
        let turn = rig_core::completion::AssistantMessage::new(
            calls.into_iter().map(AssistantContent::ToolCall).collect(),
        );
        let projected = RigMessageAdapter::assistant_from_rig(&turn);
        out.extend(projected.tool_calls.into_iter().map(ChatDelta::ToolCall));
    }
    let usage = &response.usage;
    let tokens = |value: Option<u64>| value.and_then(|value| i64::try_from(value).ok());
    if usage.input_tokens.is_some() || usage.output_tokens.is_some() || usage.total_tokens.is_some()
    {
        out.push(ChatDelta::Usage(ContextUsage {
            input_tokens: tokens(usage.input_tokens),
            cached_input_tokens: tokens(usage.cached_input_tokens),
            output_tokens: tokens(usage.output_tokens),
            total_tokens: tokens(usage.total_tokens),
            ..ContextUsage::default()
        }));
    }
    if let Some(rig_core::completion::Message::Assistant(turn)) = response.message() {
        if let Some(native) = RigMessageAdapter::assistant_from_rig(&turn).native_content {
            out.push(ChatDelta::NativeContent(native));
        }
    }
    out.push(ChatDelta::Done);
    out
}

/// The provider's rejection, with its body, when there was one.
fn http_failure(error: &ProviderError) -> Option<ProviderHttpError> {
    let (status, body) = if let Some(response) = error.provider_response() {
        (response.status?, response.body.as_str())
    } else if let ProviderError::Http(http) = error {
        (http.non_success_status()?, http.non_success_body()?)
    } else {
        return None;
    };
    if status.is_success() {
        return None;
    }
    Some(ProviderHttpError {
        status: status.as_u16(),
        body: body.chars().take(MAX_ERROR_BODY_CHARS).collect(),
    })
}

fn into_anyhow(error: ProviderError) -> anyhow::Error {
    match http_failure(&error) {
        Some(http) => http.into(),
        None => anyhow::Error::new(error),
    }
}

/// A mid-stream failure for the turn: never the provider's body, which can
/// echo the credential.
fn stream_failure(error: &ProviderError) -> String {
    match http_failure(error) {
        Some(http) => http.safe_message(),
        None => error.to_string(),
    }
}
