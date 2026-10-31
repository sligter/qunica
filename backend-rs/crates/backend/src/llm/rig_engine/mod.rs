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
use rig_core::message::ToolName;
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
    Responses(Box<OpenAIConfig>),
    Anthropic(AnthropicConfig),
    Gemini(GeminiConfig),
}

#[derive(Clone, Copy)]
enum Protocol {
    OpenAi,
    Responses,
    Anthropic,
    Gemini,
}

impl RigProvider {
    pub fn openai_responses(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> anyhow::Result<Self> {
        Self::responses_with_http(
            base_url.into(),
            api_key.into(),
            transport(&Default::default(), None)?,
        )
    }

    fn responses_with_http(
        base_url: String,
        api_key: String,
        http: DynHttpClient,
    ) -> anyhow::Result<Self> {
        let endpoint = super::openai_responses::endpoint(&base_url)?;
        let mut dialect = gateway();
        dialect.name = "openai-responses";
        // Note: resolving the entire URL once preserves tenant queries and
        // avoids appending a second /responses to a configured full endpoint.
        dialect.quirks.responses.path = "";
        let config = OpenAIConfig::with_key(&dialect, api_key)
            .with_base_url(endpoint.to_string())
            .with_system_instructions_as_messages();
        Ok(Self {
            client: Client::Responses(Box::new(config)),
            http,
        })
    }
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
            Client::Responses(config) => {
                config.as_ref().clone().connect(http).responses(name).into()
            }
            Client::Anthropic(config) => config.clone().connect(http).completion(name).into(),
            Client::Gemini(config) => config.clone().connect(http).completion(name).into(),
        }
    }

    fn protocol(&self) -> Protocol {
        match &self.client {
            Client::OpenAi(_) => Protocol::OpenAi,
            Client::Responses(_) => Protocol::Responses,
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
        "openai-responses" => Ok(Box::new(RigProvider::responses_with_http(
            base_url,
            cfg.api_key.clone(),
            http,
        )?)),
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
            Protocol::Responses => {
                item["type"] == "reasoning" && item.get("encrypted_content").is_some()
            }
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
                Protocol::Responses => ("input", ""),
                Protocol::OpenAi => return Ok(body),
            };
            let sent: Vec<_> = value[messages]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|message| {
                    if parts.is_empty() {
                        vec![message]
                    } else {
                        message[parts].as_array().into_iter().flatten().collect()
                    }
                })
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
                    Protocol::Responses => block.native_item().is_some(),
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
        Protocol::Responses => {
            extra.insert("store".into(), json!(false));
            extra.insert("include".into(), json!(["reasoning.encrypted_content"]));
            if let Some(top_p) = request.top_p {
                extra.insert("top_p".into(), json!(top_p));
            }
            if let Some(effort) = request.reasoning_effort {
                extra.insert(
                    "reasoning".into(),
                    json!({"effort": effort.as_str(), "summary": "auto"}),
                );
            }
        }
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
            Some(Err(error))
                if matches!(self.protocol(), Protocol::Responses)
                    && error.provider_response().is_some()
                    && http_failure(&error).is_none() =>
            {
                Some(Err(error))
            }
            Some(Err(error)) => return Err(into_anyhow(error)),
            first => first,
        };
        let (tx, rx) = mpsc::channel(64);
        let protocol = self.protocol();
        tokio::spawn(async move {
            relay(stream, first, tx, protocol).await;
        });
        Ok(rx)
    }
}

type RigStream = rig_core::streaming::Streamed<Completion>;
type RigItem = Option<Result<Item<StreamEvent>, ProviderError>>;

async fn relay(
    mut stream: RigStream,
    mut item: RigItem,
    tx: Sender<ChatDelta>,
    protocol: Protocol,
) {
    loop {
        let delta = match item {
            None => break,
            Some(Ok(Item::Event(StreamEvent::Text { text, .. }))) if !text.is_empty() => {
                Some(ChatDelta::Token(text))
            }
            Some(Ok(Item::Event(StreamEvent::Reasoning { text, .. }))) if !text.is_empty() => {
                Some(ChatDelta::Reasoning(text))
            }
            Some(Ok(_)) => None,
            Some(Err(error)) => {
                relay_failure(&tx, &error, protocol).await;
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
            relay_failure(&tx, &error, protocol).await;
            return;
        }
    };
    for delta in closing_deltas(&response, protocol) {
        if tx.send(delta).await.is_err() {
            return;
        }
    }
}

async fn relay_failure(tx: &Sender<ChatDelta>, error: &ProviderError, protocol: Protocol) {
    // A failed Responses terminal can still be billed. Its complete error
    // envelope is available even when Rig declines to build a model turn.
    if matches!(protocol, Protocol::Responses) {
        let payload = error
            .provider_response()
            .and_then(|response| serde_json::from_str::<Value>(&response.body).ok());
        if let Some(usage) = payload
            .as_ref()
            .and_then(|value| value.pointer("/response/usage"))
            .filter(|value| value.is_object())
        {
            let _ = tx
                .send(ChatDelta::Usage(ContextUsage {
                    input_tokens: usage["input_tokens"].as_i64(),
                    cached_input_tokens: usage["input_tokens_details"]["cached_tokens"].as_i64(),
                    output_tokens: usage["output_tokens"].as_i64(),
                    total_tokens: usage["total_tokens"].as_i64(),
                    ..ContextUsage::default()
                }))
                .await;
        }
    }
    let _ = tx.send(ChatDelta::Truncated(stream_failure(error))).await;
}

/// Tool calls, usage and the stored turn, then `Done`.
fn closing_deltas(response: &CompletionResponse, protocol: Protocol) -> Vec<ChatDelta> {
    let mut out = Vec::new();
    let turn = match response.message() {
        Some(rig_core::completion::Message::Assistant(turn)) => Some(turn),
        _ => None,
    };
    let invalid_calls = turn.as_ref().is_some_and(|turn| turn.content.iter().any(|block|
        matches!(block, AssistantContent::ToolCall(call) if call.function.invalid_arguments.is_some())))
        || (matches!(protocol, Protocol::Responses) && !responses_calls_valid(response));
    let successful = matches!(
        response.finish_reason(),
        Some(FinishReason::Stop | FinishReason::ToolCalls)
    );
    let projected = turn.as_ref().map(RigMessageAdapter::assistant_from_rig);
    // As on the native path: only a turn that finished cleanly may run its
    // calls. A cut-off turn's argument JSON is incomplete.
    if !invalid_calls && successful {
        if let Some(projected) = &projected {
            out.extend(
                projected
                    .tool_calls
                    .iter()
                    .cloned()
                    .map(ChatDelta::ToolCall),
            );
        }
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
    if matches!(protocol, Protocol::Responses) && !successful {
        out.push(ChatDelta::Truncated(
            "Responses completion was incomplete.".into(),
        ));
        return out;
    }
    if let Some(projected) = projected {
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

/// Note: Rig can repair missing ids or omit nameless calls. A repaired call
/// is not the provider's original action, so reject the entire batch.
fn responses_calls_valid(response: &CompletionResponse) -> bool {
    let raw: Vec<_> = response.raw["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "function_call")
        .collect();
    let calls: Vec<_> = response.tool_calls().collect();
    let mut ids = std::collections::BTreeSet::new();
    raw.len() == calls.len()
        && response.unfinished_calls().next().is_none()
        && raw.iter().zip(calls).all(|(item, call)| {
            let Some(id) = item["call_id"].as_str().filter(|id| !id.is_empty()) else {
                return false;
            };
            let Some(name) = item["name"].as_str().filter(|name| !name.is_empty()) else {
                return false;
            };
            let args = item["arguments"]
                .as_str()
                .and_then(|args| serde_json::from_str::<Value>(args).ok());
            ids.insert(id)
                && call.id.wire() == id
                && call.function.name.as_str() == name
                && args
                    .is_some_and(|args| args.is_object() && args == call.function.arguments_value())
        })
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
