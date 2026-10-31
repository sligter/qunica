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
use rig_core::providers::anthropic::AnthropicConfig;
use rig_core::providers::gemini::GeminiConfig;
use rig_core::providers::openai::wire::{Dialect, Quirks};
use rig_core::providers::openai::OpenAIConfig;
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
    client: Client,
    http: DynHttpClient,
}

enum Client {
    OpenAi(Box<OpenAIConfig>),
    Anthropic(AnthropicConfig),
    Gemini(GeminiConfig),
}

#[derive(Clone, Copy)]
enum Protocol {
    OpenAi,
    Anthropic,
    Gemini,
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
        let client = OpenAIConfig::with_key(&gateway(), api_key).with_base_url(base_url);
        Ok(Self {
            client: Client::OpenAi(Box::new(client)),
            http,
        })
    }

    pub fn anthropic(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self::anthropic_with_http(
            base_url.into(),
            api_key.into(),
            transport(&Default::default(), None)?,
        ))
    }

    pub fn gemini(base_url: impl Into<String>, api_key: impl Into<String>) -> anyhow::Result<Self> {
        Ok(Self::gemini_with_http(
            base_url.into(),
            api_key.into(),
            transport(&Default::default(), None)?,
        ))
    }

    fn anthropic_with_http(base_url: String, api_key: String, http: DynHttpClient) -> Self {
        Self {
            client: Client::Anthropic(
                AnthropicConfig::new(api_key)
                    .with_base_url(base_url)
                    .with_version("2023-06-01"),
            ),
            http,
        }
    }

    fn gemini_with_http(base_url: String, api_key: String, http: DynHttpClient) -> Self {
        Self {
            client: Client::Gemini(GeminiConfig::new(api_key).with_base_url(base_url)),
            http,
        }
    }

    fn model(
        &self,
        name: &str,
        explicit_empty_tools: bool,
        guard: SignedReplay,
    ) -> DynModel<Completion> {
        let mut http = if explicit_empty_tools {
            self.http.clone().with_middleware(ExplicitEmptyTools)
        } else {
            self.http.clone()
        };
        if !guard.expected.is_empty() {
            http = http.with_middleware(guard);
        }
        match &self.client {
            Client::OpenAi(config) => config.as_ref().clone().connect(http).chat(name).into(),
            Client::Anthropic(config) => config.clone().connect(http).completion(name).into(),
            Client::Gemini(config) => config.clone().connect(http).completion(name).into(),
        }
    }

    fn protocol(&self) -> Protocol {
        match &self.client {
            Client::OpenAi(_) => Protocol::OpenAi,
            Client::Anthropic(_) => Protocol::Anthropic,
            Client::Gemini(_) => Protocol::Gemini,
        }
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
        "anthropic" | "anthropic-compatible" | "anthropic_compatible" => Ok(Box::new(
            RigProvider::anthropic_with_http(base_url, cfg.api_key.clone(), http),
        )),
        "gemini" | "google" => Ok(Box::new(RigProvider::gemini_with_http(
            base_url,
            cfg.api_key.clone(),
            http,
        ))),
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

/// Note: Rig consumes additional_params.tools and drops an empty list.
/// Apply this only to requests that explicitly ask for tools: [].
struct ExplicitEmptyTools;

/// Note: Rig can discard signatures when an origin or context no longer
/// matches. Detect that at the encoded request boundary, before network I/O.
struct SignedReplay {
    protocol: Protocol,
    expected: Vec<Value>,
}

impl SignedReplay {
    fn new(request: &CompletionRequest, protocol: Protocol) -> Self {
        let expected = request
            .chat_history
            .iter()
            .filter_map(|message| {
                if let rig_core::completion::Message::Assistant(turn) = &**message {
                    Some(turn)
                } else {
                    None
                }
            })
            .flat_map(|turn| &turn.content)
            .filter_map(|block| match block {
                AssistantContent::Opaque(item) => Some(&item.item),
                _ => block.native_item(),
            })
            .filter(|item| Self::signed(item, protocol))
            .cloned()
            .collect();
        Self { protocol, expected }
    }

    fn signed(item: &Value, protocol: Protocol) -> bool {
        match protocol {
            Protocol::Anthropic => {
                item.get("signature").is_some() || item["type"] == "redacted_thinking"
            }
            Protocol::Gemini => item.get("thoughtSignature").is_some(),
            Protocol::OpenAi => false,
        }
    }

    fn comparable(mut item: Value) -> Value {
        // Gemini normalizes call ids for newer model families; the signed
        // payload, arguments and opaque metadata must otherwise stay intact.
        if let Some(call) = item.get_mut("functionCall").and_then(Value::as_object_mut) {
            call.remove("id");
        }
        item
    }
}

impl HttpMiddleware for SignedReplay {
    fn before_request_body<'a>(
        &'a self,
        _exchange: Exchange<'a>,
        _headers: &'a HeaderMap,
        body: Bytes,
    ) -> WasmBoxedFuture<'a, rig_core::http_client::Result<Bytes>> {
        Box::pin(async move {
            let value: Value =
                serde_json::from_slice(&body).map_err(rig_core::http_client::Error::instance)?;
            let (messages, parts) = match self.protocol {
                Protocol::Anthropic => ("messages", "content"),
                Protocol::Gemini => ("contents", "parts"),
                Protocol::OpenAi => return Ok(body),
            };
            let sent: Vec<_> = value[messages]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|message| message[parts].as_array().into_iter().flatten())
                .filter(|item| Self::signed(item, self.protocol))
                .cloned()
                .map(Self::comparable)
                .collect();
            let mut sent = sent.iter();
            if !self
                .expected
                .iter()
                .cloned()
                .map(Self::comparable)
                .all(|expected| sent.by_ref().any(|item| item == &expected))
            {
                return Err(rig_core::http_client::Error::instance(std::io::Error::other(
                    "Rig changed or omitted signed provider history; resume requires the original model and context",
                )));
            }
            Ok(body)
        })
    }
}

impl HttpMiddleware for ExplicitEmptyTools {
    fn before_request_body<'a>(
        &'a self,
        _exchange: Exchange<'a>,
        _headers: &'a HeaderMap,
        body: Bytes,
    ) -> WasmBoxedFuture<'a, rig_core::http_client::Result<Bytes>> {
        Box::pin(async move {
            let Ok(Value::Object(mut value)) = serde_json::from_slice::<Value>(&body) else {
                return Ok(body);
            };
            value.entry("tools").or_insert_with(|| json!([]));
            Ok(serde_json::to_vec(&value).map(Bytes::from).unwrap_or(body))
        })
    }
}

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

fn completion_request(
    request: &ChatRequest,
    protocol: Protocol,
) -> anyhow::Result<CompletionRequest> {
    let mut history = adapter::provider_history(request, protocol)?;
    // Note: the per-model passback switch decides whether past reasoning is
    // sent, as on the native path; Rig would replay a same-origin turn's
    // reasoning unconditionally.
    for message in &mut history {
        if let rig_core::completion::Message::Assistant(turn) = message {
            turn.content.retain(|block| match block {
                AssistantContent::Reasoning(reasoning) => match protocol {
                    Protocol::OpenAi => {
                        request.reasoning_passback && !reasoning.text.trim().is_empty()
                    }
                    Protocol::Anthropic => {
                        request.reasoning_effort.is_some() && block.native_item().is_some()
                    }
                    Protocol::Gemini => true,
                },
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
    // The level as chosen, as the native path sends it: Rig's mapping would
    // round levels the catalog does not list for an unknown model.
    match protocol {
        Protocol::OpenAi => {
            if let Some(top_p) = request.top_p {
                extra.insert("top_p".into(), json!(top_p));
            }
            if let Some(effort) = request.reasoning_effort {
                extra.insert("reasoning_effort".into(), json!(effort.as_str()));
            }
        }
        Protocol::Anthropic => {
            let budget = request
                .reasoning_effort
                .map(|effort| effort.thinking_budget_tokens().max(1024));
            rig.max_tokens = Some(budget.map_or(4096, |value| value.saturating_add(4096)) as u64);
            if let Some(budget) = budget {
                rig.temperature = None;
                extra.insert(
                    "thinking".into(),
                    json!({"type": "enabled", "budget_tokens": budget}),
                );
            } else if let Some(top_p) = request.top_p {
                extra.insert("top_p".into(), json!(top_p));
            }
        }
        Protocol::Gemini => {
            let mut config = serde_json::Map::new();
            if let Some(top_p) = request.top_p {
                config.insert("topP".into(), json!(top_p));
            }
            if let Some(effort) = request.reasoning_effort {
                config.insert(
                    "thinkingConfig".into(),
                    json!({"thinkingBudget": effort.thinking_budget_tokens()}),
                );
            }
            if !config.is_empty() {
                extra.insert("generationConfig".into(), Value::Object(config));
            }
        }
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
        let rig_request = completion_request(&request, self.protocol())?;
        let guard = SignedReplay::new(&rig_request, self.protocol());
        let mut stream = self
            .model(
                &request.model,
                request.include_empty_tools && request.tools.is_empty(),
                guard,
            )
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
    let invalid_calls = calls
        .iter()
        .any(|call| call.function.invalid_arguments.is_some());
    // As on the native path: only a turn that finished cleanly may run its
    // calls. A cut-off turn's argument JSON is incomplete.
    if !invalid_calls
        && matches!(
            response.finish_reason(),
            Some(FinishReason::Stop | FinishReason::ToolCalls)
        )
    {
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
    if invalid_calls {
        // Note: Rig represents malformed argument text alongside a completion.
        // Keep charged usage, but never commit or dispatch any of its tools.
        out.push(ChatDelta::Truncated(
            "The provider returned invalid tool arguments.".into(),
        ));
        return out;
    }
    if let Some(rig_core::completion::Message::Assistant(turn)) = response.message() {
        let projected = RigMessageAdapter::assistant_from_rig(&turn);
        if let Some(signature) = projected.reasoning_signature {
            out.push(ChatDelta::ReasoningSignature(signature));
        }
        if let Some(native) = projected.native_content {
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
