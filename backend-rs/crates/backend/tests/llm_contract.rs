//! Provider contract tests.
//!
//! Each test spins up a local HTTP server that replays a canned server-sent
//! event body, points a provider at it, and asserts that the provider maps the
//! provider-specific wire format onto the neutral [`ChatDelta`] vocabulary.
//! No live external API is contacted.

use std::sync::Arc;

use axum::{body::Body, http::header, response::IntoResponse, Router};
use qunica_backend::llm::{
    build_provider, AnthropicProvider, ChatDelta, ChatMessage, ChatRequest, GeminiProvider,
    LlmProvider, OpenAiCompatibleProvider, OpenAiResponsesProvider, ProviderConfig,
    ProviderHttpError, ReasoningEffort, ToolCall, ToolDefinition,
};
use qunica_domain::runtime::ChatContentPart;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;

/// The engines a contract test runs against: every OpenAI-compatible case
/// must hold for the native provider and, when built in, for Rig.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Engine {
    Native,
    #[cfg(feature = "rig-engine")]
    Rig,
}

impl Engine {
    fn is_rig(self) -> bool {
        self != Engine::Native
    }
}

const ENGINES: &[Engine] = &[
    Engine::Native,
    #[cfg(feature = "rig-engine")]
    Engine::Rig,
];

fn openai(engine: Engine, url: String) -> Box<dyn LlmProvider> {
    match engine {
        Engine::Native => Box::new(OpenAiCompatibleProvider::new(url, "test-key")),
        #[cfg(feature = "rig-engine")]
        Engine::Rig => Box::new(
            qunica_backend::llm::rig_engine::RigProvider::openai_compatible(url, "test-key")
                .expect("rig provider"),
        ),
    }
}

/// The smallest complete chat-completions reply, for cases that only inspect
/// the request. Rig rejects a reply that is a bare `[DONE]`: it has no
/// finish, so nothing says the answer is whole.
const OPENAI_STOP: &str =
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

/// One `#[tokio::test]` per case, running it once per engine.
macro_rules! on_every_engine {
    ($($case:ident),* $(,)?) => {
        mod on_every_engine {
            $(
                #[tokio::test]
                async fn $case() {
                    for &engine in super::ENGINES {
                        eprintln!("engine: {engine:?}");
                        super::$case(engine).await;
                    }
                }
            )*
        }
    };
}

/// Start a single-shot HTTP server that responds to every request with `body`
/// as an event stream. Returns the base URL (e.g. `http://127.0.0.1:54321`).
async fn fake_server(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    let app = Router::new().fallback(move || async move {
        ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
    });

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake provider");
    });

    format!("http://{addr}")
}

/// Start a server that answers with valid headers and a partial body, then
/// drops the connection with the promised bytes unsent — what a gateway idle
/// timeout looks like from the client side.
async fn truncating_server(partial_body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut discard = [0u8; 8192];
            let _ = socket.read(&mut discard).await;
            let head = "HTTP/1.1 200 OK\r\n\
                        Content-Type: text/event-stream\r\n\
                        Content-Length: 65536\r\n\r\n";
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(partial_body.as_bytes()).await;
            let _ = socket.flush().await;
            // Dropping `socket` here cuts the body short of Content-Length.
        }
    });

    format!("http://{addr}")
}

async fn capture_server(body: &'static str) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let captures = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback({
        let captures = Arc::clone(&captures);
        move |request: axum::http::Request<Body>| {
            let captures = Arc::clone(&captures);
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .expect("request body");
                let value = serde_json::from_slice::<Value>(&bytes).expect("json request");
                captures.lock().await.push(value);
                ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
            }
        }
    });

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake provider");
    });

    (format!("http://{addr}"), captures)
}

/// A minimal request; the canned response is independent of these fields.
fn request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![ChatMessage::text("user", "hi")],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: None,
    }
}

fn image_request(role: &str) -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![ChatMessage::with_parts(
            role,
            vec![
                ChatContentPart::text("describe this image"),
                ChatContentPart::image("image/png", "AQID"),
            ],
        )],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: None,
    }
}

fn continuation_request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![
            ChatMessage::text("system", "Use tools carefully."),
            ChatMessage::text("user", "read the file"),
            ChatMessage::assistant_tool_calls(
                "Checking.",
                vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "Read".to_string(),
                    args: json!({ "file_path": "note.txt" }),
                    provider_metadata: None,
                }],
            ),
            ChatMessage::tool_result("call_1", "Read", "file body"),
        ],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        // A continuation declares the tools it called, as the runtime's do.
        // Rig sends tool history to a request without tools as text.
        tools: vec![ToolDefinition {
            name: "Read".to_string(),
            description: "Read a file".to_string(),
            input_schema: json!({"type": "object", "properties": {"file_path": {"type": "string"}}}),
        }],
        reasoning_effort: None,
    }
}

fn parallel_continuation_request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![
            ChatMessage::text("user", "read two files"),
            ChatMessage::assistant_tool_calls(
                "",
                vec![
                    ToolCall {
                        id: "call_1".to_string(),
                        name: "Read".to_string(),
                        args: json!({ "file_path": "a.txt" }),
                        provider_metadata: None,
                    },
                    ToolCall {
                        id: "call_2".to_string(),
                        name: "Read".to_string(),
                        args: json!({ "file_path": "b.txt" }),
                        provider_metadata: None,
                    },
                ],
            ),
            ChatMessage::tool_result("call_1", "Read", "a body"),
            ChatMessage::tool_result("call_2", "Read", "b body"),
        ],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: None,
    }
}

fn gemini_thought_signature_request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![
            ChatMessage::text("user", "search"),
            ChatMessage::assistant_tool_calls(
                "",
                vec![ToolCall {
                    id: "search".to_string(),
                    name: "search".to_string(),
                    args: json!({ "q": "rust" }),
                    provider_metadata: Some(json!({
                        "thoughtSignature": "sig-123"
                    })),
                }],
            ),
            ChatMessage::tool_result("search", "search", "{\"ok\":true}"),
        ],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: None,
    }
}

/// Drain every delta the provider emits.
async fn collect(mut rx: Receiver<ChatDelta>) -> Vec<ChatDelta> {
    let mut deltas = Vec::new();
    while let Some(delta) = rx.recv().await {
        deltas.push(delta);
    }
    deltas
}

fn tokens(deltas: &[ChatDelta]) -> Vec<String> {
    deltas
        .iter()
        .filter_map(|d| match d {
            ChatDelta::Token(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn reasoning(deltas: &[ChatDelta]) -> Vec<String> {
    deltas
        .iter()
        .filter_map(|d| match d {
            ChatDelta::Reasoning(r) => Some(r.clone()),
            _ => None,
        })
        .collect()
}

fn tool_calls(deltas: &[ChatDelta]) -> Vec<(String, String, serde_json::Value)> {
    deltas
        .iter()
        .filter_map(|d| match d {
            ChatDelta::ToolCall(tc) => Some((tc.id.clone(), tc.name.clone(), tc.args.clone())),
            _ => None,
        })
        .collect()
}

fn usages(deltas: &[ChatDelta]) -> Vec<(Option<i64>, Option<i64>, Option<i64>)> {
    deltas
        .iter()
        .filter_map(|d| match d {
            ChatDelta::Usage(u) => Some((u.input_tokens, u.output_tokens, u.total_tokens)),
            _ => None,
        })
        .collect()
}

fn ends_with_done(deltas: &[ChatDelta]) -> bool {
    matches!(deltas.last(), Some(ChatDelta::Done))
}

async fn llm_contract_openai_maps_content_to_tokens(engine: Engine) {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                data: [DONE]\n\n";
    let url = fake_server(body).await;
    let provider = openai(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hello", " world"]);
    assert!(ends_with_done(&deltas));
}

async fn llm_contract_openai_maps_reasoning_content_and_reasoning_to_reasoning(engine: Engine) {
    // `reasoning_content` (DeepSeek-style) and `reasoning` (OpenAI-style) both
    // map to ChatDelta::Reasoning.
    let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"step 1\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"reasoning\":\"step 2\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\n\
                data: [DONE]\n\n";
    let url = fake_server(body).await;
    let provider = openai(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(reasoning(&deltas), vec!["step 1", "step 2"]);
    assert_eq!(tokens(&deltas), vec!["answer"]);
}

async fn llm_contract_openai_maps_usage_and_streamed_tool_calls(engine: Engine) {
    let body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Paris\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
                data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\n\
                data: [DONE]\n\n";
    let url = fake_server(body).await;
    let provider = openai(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call_1");
    assert_eq!(calls[0].1, "get_weather");
    assert_eq!(calls[0].2, serde_json::json!({ "city": "Paris" }));

    assert_eq!(usages(&deltas), vec![(Some(12), Some(7), Some(19))]);
}

async fn llm_contract_openai_emits_tool_calls_finished_with_stop(engine: Engine) {
    // Some OpenAI-compatible gateways close a tool-calling turn with `stop`
    // instead of `tool_calls`. The buffered call still has to be emitted, or the
    // agent's round ends with no tool result and no text.
    let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\"}\"}}]},\"finish_reason\":\"stop\"}]}\n\n\
                data: [DONE]\n\n";
    let url = fake_server(body).await;
    let provider = openai(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "Read");
    assert_eq!(calls[0].2, serde_json::json!({ "file_path": "note.txt" }));
}

async fn llm_contract_openai_drops_tool_calls_truncated_by_the_token_limit(engine: Engine) {
    // `length` means the arguments were cut mid-JSON, so the call is incomplete
    // and must not be handed to the tool executor.
    let body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\"}}]},\"finish_reason\":\"length\"}]}\n\n\
                data: [DONE]\n\n";
    let url = fake_server(body).await;
    let provider = openai(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert!(tool_calls(&deltas).is_empty());
}

async fn llm_contract_openai_reports_a_cut_stream_as_truncated_not_done(engine: Engine) {
    // The provider hung up mid-body. The deltas that did arrive are kept, but
    // the stream must close with `Truncated` — ending it with `Done` would make
    // a dropped connection indistinguishable from a model that stopped talking.
    let url =
        truncating_server("data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n").await;
    let provider = openai(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hel"]);
    assert!(matches!(deltas.last(), Some(ChatDelta::Truncated(_))));
    assert!(!ends_with_done(&deltas));
}

async fn llm_contract_openai_serializes_tool_continuation_messages(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);

    let _ = collect(provider.stream(continuation_request()).await.unwrap()).await;

    let captured = captures.lock().await;
    let messages = captured[0]["messages"].as_array().unwrap();
    assert_eq!(messages[2]["role"], "assistant");
    assert_eq!(messages[2]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[2]["tool_calls"][0]["function"]["name"], "Read");
    assert_eq!(
        messages[2]["tool_calls"][0]["function"]["arguments"],
        "{\"file_path\":\"note.txt\"}"
    );
    assert_eq!(messages[3]["role"], "tool");
    assert_eq!(messages[3]["tool_call_id"], "call_1");
    // `name` is not part of OpenAI's tool message; Rig leaves it out.
    let name = if engine.is_rig() {
        Value::Null
    } else {
        json!("Read")
    };
    assert_eq!(messages[3]["name"], name);
    assert_eq!(messages[3]["content"], "file body");
}

async fn llm_contract_openai_sends_reasoning_back_only_when_the_model_asks_for_it(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);

    let mut off = continuation_request();
    off.messages[2] = off.messages[2]
        .clone()
        .with_reasoning("weighing the options");
    let _ = collect(provider.stream(off.clone()).await.unwrap()).await;

    let mut on = off;
    on.reasoning_passback = true;
    let _ = collect(provider.stream(on).await.unwrap()).await;

    let captured = captures.lock().await;
    // Off is the stored default; gateways that require the field are healed by
    // the runtime's one-shot retry rather than by echoing it unconditionally.
    assert!(
        captured[0]["messages"][2]
            .get("reasoning_content")
            .is_none(),
        "{}",
        captured[0]["messages"][2]
    );
    // On is what a gateway running the model in thinking mode demands: without
    // this the tool-calling turn comes back as HTTP 400.
    assert_eq!(
        captured[1]["messages"][2]["reasoning_content"],
        "weighing the options"
    );
    assert_eq!(
        captured[1]["messages"][2]["tool_calls"][0]["id"], "call_1",
        "the reasoning rides alongside the tool call, not instead of it"
    );
    assert!(
        captured[1]["messages"][3]
            .get("reasoning_content")
            .is_none(),
        "a tool result has no reasoning of its own: {}",
        captured[1]["messages"][3]
    );
}

async fn llm_contract_openai_omits_blank_reasoning_even_with_passback_on(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);
    let mut request = continuation_request();
    request.reasoning_passback = true;
    request.messages[2] = request.messages[2].clone().with_reasoning("   ");

    let _ = collect(provider.stream(request).await.unwrap()).await;

    let captured = captures.lock().await;
    assert!(
        captured[0]["messages"][2]
            .get("reasoning_content")
            .is_none(),
        "an empty reasoning field is worse than no field: {}",
        captured[0]["messages"][2]
    );
}

async fn llm_contract_openai_omits_absent_temperature(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);
    let mut request = request();
    request.temperature = None;

    let _ = collect(provider.stream(request).await.unwrap()).await;

    let captured = captures.lock().await;
    assert!(captured[0].get("temperature").is_none(), "{}", captured[0]);
    assert!(captured[0].get("top_p").is_none(), "{}", captured[0]);
}

async fn llm_contract_openai_serializes_sampling_as_f64(engine: Engine) {
    // The value comes from JSON configuration. Carrying it as f32 and
    // re-serializing it emitted `0.949999988079071` for a configured 0.95.
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);
    let mut request = request();
    request.temperature = Some(0.95);
    request.top_p = Some(0.8);

    let _ = collect(provider.stream(request).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(captured[0]["temperature"], json!(0.95), "{}", captured[0]);
    assert_eq!(captured[0]["top_p"], json!(0.8), "{}", captured[0]);
}

async fn llm_contract_openai_serializes_image_parts_and_preserves_text_only_shape(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);

    let _ = collect(provider.stream(image_request("user")).await.unwrap()).await;
    let _ = collect(provider.stream(request()).await.unwrap()).await;

    let captured = captures.lock().await;
    let mut image_url = json!({ "url": "data:image/png;base64,AQID" });
    // Rig spells out the default detail level; the native path leaves it to
    // the provider, whose default is the same `auto`.
    if engine.is_rig() {
        image_url["detail"] = json!("auto");
    }
    assert_eq!(
        captured[0]["messages"][0]["content"],
        json!([
            { "type": "text", "text": "describe this image" },
            { "type": "image_url", "image_url": image_url }
        ])
    );
    assert_eq!(captured[1]["messages"][0]["content"], "hi");
}

async fn llm_contract_openai_serializes_system_image_parts(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);

    // Rig's system message is text only. The runtime never puts an image
    // there, so the Rig engine refuses one up front instead of dropping it.
    if engine.is_rig() {
        assert!(provider.stream(image_request("system")).await.is_err());
        assert!(captures.lock().await.is_empty());
        return;
    }
    let _ = collect(provider.stream(image_request("system")).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(captured[0]["messages"][0]["role"], "system");
    assert_eq!(
        captured[0]["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,AQID"
    );
}

#[tokio::test]
async fn llm_contract_anthropic_maps_text_and_thinking_deltas() {
    let body = "event: message_start\n\
                data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25}}}\n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"pondering\"}}\n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
                event: message_delta\n\
                data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":9}}\n\
                event: message_stop\n\
                data: {\"type\":\"message_stop\"}\n";
    let url = fake_server(body).await;
    let provider = AnthropicProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hello"]);
    assert_eq!(reasoning(&deltas), vec!["pondering"]);
    // Anthropic reports the input count once in `message_start` and only the
    // output count in `message_delta`, so the closing usage carries the
    // remembered input forward to report a turn total.
    assert_eq!(
        usages(&deltas),
        vec![(Some(25), None, None), (Some(25), Some(9), Some(34))]
    );
    assert!(ends_with_done(&deltas));
}

#[tokio::test]
async fn llm_contract_anthropic_maps_streamed_tool_use_block() {
    let body = "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"lookup\"}}\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\"}}\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"rust\\\"}\"}}\n\
                data: {\"type\":\"content_block_stop\",\"index\":0}\n";
    let url = fake_server(body).await;
    let provider = AnthropicProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "toolu_1");
    assert_eq!(calls[0].1, "lookup");
    assert_eq!(calls[0].2, serde_json::json!({ "q": "rust" }));
}

#[tokio::test]
async fn llm_contract_anthropic_serializes_system_and_tool_continuation_messages() {
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let _ = collect(provider.stream(continuation_request()).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(captured[0]["system"], "Use tools carefully.");
    let messages = captured[0]["messages"].as_array().unwrap();
    assert!(messages.iter().all(|message| message["role"] != "system"));
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "text");
    assert_eq!(messages[1]["content"][1]["type"], "tool_use");
    assert_eq!(messages[1]["content"][1]["id"], "call_1");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[2]["content"][0]["tool_use_id"], "call_1");
}

#[tokio::test]
async fn llm_contract_anthropic_serializes_image_parts_and_preserves_text_only_shape() {
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let _ = collect(provider.stream(image_request("user")).await.unwrap()).await;
    let _ = collect(provider.stream(request()).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(
        captured[0]["messages"][0]["content"],
        json!([
            { "type": "text", "text": "describe this image" },
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "AQID"
                }
            }
        ])
    );
    assert_eq!(captured[1]["messages"][0]["content"], "hi");
}

#[tokio::test]
async fn llm_contract_anthropic_serializes_system_image_parts() {
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let _ = collect(provider.stream(image_request("system")).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(captured[0]["system"][0]["type"], "text");
    assert_eq!(captured[0]["system"][1]["source"]["type"], "base64");
    assert_eq!(captured[0]["system"][1]["source"]["data"], "AQID");
}

#[tokio::test]
async fn llm_contract_anthropic_coalesces_parallel_tool_results() {
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let _ = collect(
        provider
            .stream(parallel_continuation_request())
            .await
            .unwrap(),
    )
    .await;

    let captured = captures.lock().await;
    let messages = captured[0]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][0]["id"], "call_1");
    assert_eq!(messages[1]["content"][1]["type"], "tool_use");
    assert_eq!(messages[1]["content"][1]["id"], "call_2");
    assert_eq!(messages[2]["role"], "user");
    let content = messages[2]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "tool_result");
    assert_eq!(content[0]["tool_use_id"], "call_1");
    assert_eq!(content[0]["content"], "a body");
    assert_eq!(content[1]["type"], "tool_result");
    assert_eq!(content[1]["tool_use_id"], "call_2");
    assert_eq!(content[1]["content"], "b body");
}

#[tokio::test]
async fn llm_contract_gemini_maps_text_and_usage() {
    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}\n\
                data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" there\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":3,\"totalTokenCount\":8}}\n";
    let url = fake_server(body).await;
    let provider = GeminiProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hello", " there"]);
    assert_eq!(usages(&deltas), vec![(Some(5), Some(3), Some(8))]);
    assert!(ends_with_done(&deltas));
}

#[tokio::test]
async fn llm_contract_gemini_maps_function_call() {
    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"search\",\"args\":{\"q\":\"rust\"}}}]}}]}\n";
    let url = fake_server(body).await;
    let provider = GeminiProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "search");
    assert_eq!(calls[0].2, serde_json::json!({ "q": "rust" }));
}

#[tokio::test]
async fn llm_contract_gemini_preserves_function_call_thought_signature() {
    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thoughtSignature\":\"sig-123\",\"functionCall\":{\"name\":\"search\",\"args\":{\"q\":\"rust\"}}}]}}]}\n";
    let url = fake_server(body).await;
    let provider = GeminiProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let call = deltas
        .iter()
        .find_map(|delta| match delta {
            ChatDelta::ToolCall(call) => Some(call),
            _ => None,
        })
        .expect("tool call");
    assert_eq!(
        call.provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("thoughtSignature"))
            .and_then(Value::as_str),
        Some("sig-123")
    );
}

#[tokio::test]
async fn llm_contract_gemini_serializes_function_response_continuation_messages() {
    let (url, captures) = capture_server("data: {}\n").await;
    let provider = GeminiProvider::new(url, "test-key");

    let _ = collect(provider.stream(continuation_request()).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(
        captured[0]["systemInstruction"]["parts"][0]["text"],
        "Use tools carefully."
    );
    let contents = captured[0]["contents"].as_array().unwrap();
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[1]["parts"][0]["text"], "Checking.");
    assert_eq!(contents[1]["parts"][1]["functionCall"]["name"], "Read");
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "Read");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["result"],
        "file body"
    );
}

#[tokio::test]
async fn llm_contract_gemini_serializes_image_parts_and_preserves_text_only_shape() {
    let (url, captures) = capture_server("data: {}\n").await;
    let provider = GeminiProvider::new(url, "test-key");

    let _ = collect(provider.stream(image_request("user")).await.unwrap()).await;
    let _ = collect(provider.stream(request()).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(
        captured[0]["contents"][0]["parts"],
        json!([
            { "text": "describe this image" },
            { "inlineData": { "mimeType": "image/png", "data": "AQID" } }
        ])
    );
    assert_eq!(
        captured[1]["contents"][0]["parts"],
        json!([{ "text": "hi" }])
    );
}

#[tokio::test]
async fn llm_contract_gemini_serializes_system_image_parts() {
    let (url, captures) = capture_server("data: {}\n").await;
    let provider = GeminiProvider::new(url, "test-key");

    let _ = collect(provider.stream(image_request("system")).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(
        captured[0]["systemInstruction"]["parts"][1]["inlineData"]["mimeType"],
        "image/png"
    );
    assert_eq!(
        captured[0]["systemInstruction"]["parts"][1]["inlineData"]["data"],
        "AQID"
    );
}

#[tokio::test]
async fn llm_contract_gemini_replays_function_call_thought_signature() {
    let (url, captures) = capture_server("data: {}\n").await;
    let provider = GeminiProvider::new(url, "test-key");

    let _ = collect(
        provider
            .stream(gemini_thought_signature_request())
            .await
            .unwrap(),
    )
    .await;

    let captured = captures.lock().await;
    let contents = captured[0]["contents"].as_array().unwrap();
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "search");
    assert_eq!(contents[1]["parts"][0]["thoughtSignature"], "sig-123");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["name"],
        "search"
    );
}

// ---------------------------------------------------------------------------
// Reasoning effort: one five-level abstraction, three provider dialects
// ---------------------------------------------------------------------------

fn effort_request(effort: Option<ReasoningEffort>) -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![ChatMessage::text("user", "think")],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: effort,
    }
}

async fn captured_body(
    provider: &dyn LlmProvider,
    captures: &Arc<Mutex<Vec<Value>>>,
    effort: Option<ReasoningEffort>,
) -> Value {
    let _ = collect(provider.stream(effort_request(effort)).await.unwrap()).await;
    let body = captures.lock().await.last().cloned().expect("a request");
    captures.lock().await.clear();
    body
}

async fn llm_contract_openai_maps_effort_to_its_own_enum(engine: Engine) {
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);

    for (effort, expected) in [
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "xhigh"),
        (ReasoningEffort::Max, "max"),
    ] {
        let body = captured_body(&*provider, &captures, Some(effort)).await;
        assert_eq!(body["reasoning_effort"], json!(expected));
    }
}

async fn llm_contract_openai_never_sends_a_thinking_budget(engine: Engine) {
    // This endpoint's vocabulary for thinking is the level. A budget key is an
    // unknown parameter here — at best ignored, at worst a rejected request.
    let (url, captures) = capture_server(OPENAI_STOP).await;
    let provider = openai(engine, url);

    for effort in ReasoningEffort::ALL {
        let body = captured_body(&*provider, &captures, Some(effort)).await;
        for key in ["thinking", "thinking_budget", "budget_tokens", "reasoning"] {
            assert!(body.get(key).is_none(), "{key} in {body}");
        }
        assert!(body.get("max_tokens").is_none(), "{body}");
    }
}

#[tokio::test]
async fn llm_contract_anthropic_maps_effort_to_a_thinking_budget() {
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let mut previous = 0;
    for effort in ReasoningEffort::ALL {
        let body = captured_body(&provider, &captures, Some(effort)).await;
        assert_eq!(body["thinking"]["type"], "enabled");
        let budget = body["thinking"]["budget_tokens"].as_i64().unwrap();
        // Anthropic rejects a budget that is not below max_tokens, so the
        // request has to make room for the answer above whatever it thinks.
        let max_tokens = body["max_tokens"].as_i64().unwrap();
        assert!(
            budget < max_tokens,
            "budget {budget} vs max_tokens {max_tokens}"
        );
        // 1024 is the shallowest budget the API accepts.
        assert!(budget >= 1024, "budget {budget} below the provider minimum");
        assert!(budget > previous, "effort must increase the budget");
        previous = budget;
    }
}

#[tokio::test]
async fn llm_contract_anthropic_drops_temperature_while_thinking() {
    // Extended thinking fixes sampling: Anthropic rejects any temperature but 1
    // while it is on, and `effort_request` asks for 0.0.
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let body = captured_body(&provider, &captures, Some(ReasoningEffort::High)).await;
    assert!(body.get("temperature").is_none(), "{body}");

    // Without thinking the setting applies as asked — and is absent rather
    // than null when there is none.
    let body = captured_body(&provider, &captures, None).await;
    assert_eq!(body["temperature"], json!(0.0));
}

#[tokio::test]
async fn llm_contract_anthropic_sends_signed_thinking_back_with_its_tool_calls() {
    // With thinking on, Anthropic requires the assistant turn that made the
    // tool calls to lead with the signed thinking block that produced them;
    // tool results following a bare text block are rejected outright.
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let mut request = effort_request(Some(ReasoningEffort::High));
    request.messages = vec![
        ChatMessage::text("user", "search"),
        ChatMessage::assistant_tool_calls(
            "looking",
            vec![ToolCall {
                id: "call-1".to_string(),
                name: "search".to_string(),
                args: json!({ "q": "rust" }),
                provider_metadata: None,
            }],
        )
        .with_reasoning("I should search.")
        .with_reasoning_signature(Some("sig-abc".to_string())),
        ChatMessage::tool_result("call-1", "search", "{\"ok\":true}"),
    ];
    let _ = collect(provider.stream(request).await.unwrap()).await;
    let body = captures.lock().await.last().cloned().expect("a request");

    let content = body["messages"][1]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking", "{body}");
    assert_eq!(content[0]["thinking"], "I should search.");
    assert_eq!(content[0]["signature"], "sig-abc");
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[2]["type"], "tool_use");
}

#[tokio::test]
async fn llm_contract_anthropic_omits_thinking_it_cannot_prove() {
    // Reasoning replayed from stored history has no signature, and the
    // provider verifies one against the other — so an unsigned block is a
    // rejected request rather than a preserved thought. The same block is
    // equally invalid when thinking is off.
    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let provider = AnthropicProvider::new(url, "test-key");

    let assistant = ChatMessage::assistant_tool_calls(
        "looking",
        vec![ToolCall {
            id: "call-1".to_string(),
            name: "search".to_string(),
            args: json!({}),
            provider_metadata: None,
        }],
    )
    .with_reasoning("recovered from the database");

    let mut unsigned = effort_request(Some(ReasoningEffort::High));
    unsigned.messages = vec![ChatMessage::text("user", "search"), assistant.clone()];
    let _ = collect(provider.stream(unsigned).await.unwrap()).await;

    let mut thinking_off = effort_request(None);
    thinking_off.messages = vec![
        ChatMessage::text("user", "search"),
        assistant.with_reasoning_signature(Some("sig-abc".to_string())),
    ];
    let _ = collect(provider.stream(thinking_off).await.unwrap()).await;

    let bodies = captures.lock().await.clone();
    for body in bodies {
        assert_eq!(body["messages"][1]["content"][0]["type"], "text", "{body}");
    }
}

#[tokio::test]
async fn llm_contract_anthropic_surfaces_the_signature_over_its_thinking() {
    let url = fake_server(concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"step\"}}\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-abc\"}}\n",
    ))
    .await;
    let provider = AnthropicProvider::new(url, "test-key");

    let deltas = collect(
        provider
            .stream(effort_request(Some(ReasoningEffort::High)))
            .await
            .unwrap(),
    )
    .await;

    assert_eq!(reasoning(&deltas), vec!["step".to_string()]);
    assert!(
        deltas
            .iter()
            .any(|delta| matches!(delta, ChatDelta::ReasoningSignature(s) if s == "sig-abc")),
        "{deltas:?}"
    );
}

#[tokio::test]
async fn llm_contract_anthropic_reports_a_mid_stream_error_as_truncation() {
    // The status was 200 and the body began normally, so nothing else marks
    // this as a failure: ending on `Done` would pass a dropped answer off as a
    // model that chose to say nothing.
    let url = fake_server(concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n",
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n",
    ))
    .await;
    let provider = AnthropicProvider::new(url, "test-key");

    let deltas = collect(provider.stream(effort_request(None)).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["partial".to_string()]);
    assert!(
        deltas
            .iter()
            .any(|delta| matches!(delta, ChatDelta::Truncated(reason) if reason == "Overloaded")),
        "{deltas:?}"
    );
}

#[tokio::test]
async fn llm_contract_gemini_maps_effort_to_a_thinking_config() {
    let (url, captures) = capture_server("data: {}\n").await;
    let provider = GeminiProvider::new(url, "test-key");

    let body = captured_body(&provider, &captures, Some(ReasoningEffort::High)).await;
    let thinking = &body["generationConfig"]["thinkingConfig"];
    assert!(thinking.is_object(), "{body}");
    assert!(thinking["thinkingBudget"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn llm_contract_absent_effort_omits_the_key_entirely() {
    // A `null` is not the same as absence: a strict provider rejects the key
    // outright, and an older gateway may not know it at all.
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let openai = OpenAiCompatibleProvider::new(url, "test-key");
    let body = captured_body(&openai, &captures, None).await;
    assert!(body.get("reasoning_effort").is_none(), "{body}");

    let (url, captures) = capture_server("data: {\"type\":\"message_stop\"}\n").await;
    let anthropic = AnthropicProvider::new(url, "test-key");
    let body = captured_body(&anthropic, &captures, None).await;
    assert!(body.get("thinking").is_none(), "{body}");

    let (url, captures) = capture_server("data: {}\n").await;
    let gemini = GeminiProvider::new(url, "test-key");
    let body = captured_body(&gemini, &captures, None).await;
    assert!(
        body["generationConfig"].get("thinkingConfig").is_none(),
        "{body}"
    );
}

// Responses uses complete output items for calls and a required terminal event.
const RESPONSES_TEXT: &str = concat!(
    "event: response.created\ndata: {\"type\":\"response.created\"}\r\n\r\n",
    "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Checking\"}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好\"}\n\n",
    "data: {\"type\":\"response.output_text.done\",\"text\":\"你好\"}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"output\":[],\"usage\":{\"input_tokens\":100,\"input_tokens_details\":{\"cached_tokens\":80},\"output_tokens\":5,\"total_tokens\":105}}}",
);

#[tokio::test]
async fn responses_streams_text_reasoning_and_usage_without_duplicate_done() {
    let url = fake_server(RESPONSES_TEXT).await;
    let deltas = collect(
        OpenAiResponsesProvider::new(url, "test-key")
            .stream(request())
            .await
            .unwrap(),
    )
    .await;
    assert!(matches!(&deltas[0], ChatDelta::Reasoning(text) if text == "Checking"));
    assert!(matches!(&deltas[1], ChatDelta::Token(text) if text == "你好"));
    let ChatDelta::Usage(usage) = &deltas[2] else {
        panic!("expected usage")
    };
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cached_input_tokens, Some(80));
    assert_eq!(usage.output_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(105));
    assert!(matches!(deltas[3], ChatDelta::Done));
    assert_eq!(deltas.len(), 4);
}

#[tokio::test]
async fn responses_encodes_history_images_tools_and_optional_settings() {
    let (url, captures) = capture_server(RESPONSES_TEXT).await;
    let provider = OpenAiResponsesProvider::new(url, "test-key");
    let mut req = continuation_request();
    req.messages
        .insert(2, image_request("user").messages.remove(0));
    req.temperature = None;
    req.reasoning_effort = Some(ReasoningEffort::XHigh);
    req.tools = vec![qunica_backend::llm::ToolDefinition {
        name: "Read".into(),
        description: "Read a file".into(),
        input_schema: json!({"type":"object","properties":{"file_path":{"type":"string"}}}),
    }];
    collect(provider.stream(req).await.unwrap()).await;
    let body = captures.lock().await[0].clone();
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["reasoning"]["effort"], "xhigh");
    for key in [
        "messages",
        "stream_options",
        "temperature",
        "reasoning_effort",
    ] {
        assert!(body.get(key).is_none(), "unexpected {key}");
    }
    assert_eq!(body["input"][0]["role"], "system");
    assert_eq!(body["input"][2]["content"][0]["type"], "input_text");
    assert_eq!(
        body["input"][2]["content"][1],
        json!({"type":"input_image","image_url":"data:image/png;base64,AQID"})
    );
    assert_eq!(
        body["input"][3],
        json!({"role":"assistant","content":"Checking."})
    );
    assert_eq!(body["input"][4]["type"], "function_call");
    assert_eq!(body["input"][4]["call_id"], "call_1");
    assert_eq!(
        serde_json::from_str::<Value>(body["input"][4]["arguments"].as_str().unwrap()).unwrap(),
        json!({"file_path":"note.txt"})
    );
    assert_eq!(body["input"][5]["type"], "function_call_output");
    assert_eq!(body["input"][5]["call_id"], "call_1");
    assert_eq!(body["tools"][0]["name"], "Read");
    assert_eq!(body["tools"][0]["strict"], false);
    assert!(body["tools"][0].get("function").is_none());
    let mut req = request();
    req.temperature = Some(0.5);
    collect(provider.stream(req.clone()).await.unwrap()).await;
    req.include_empty_tools = true;
    collect(provider.stream(req).await.unwrap()).await;
    let captures = captures.lock().await;
    assert_eq!(captures[1]["temperature"], 0.5);
    assert!(captures[1].get("tools").is_none());
    assert!(captures[1].get("reasoning").is_none());
    assert_eq!(captures[2]["tools"], json!([]));
}

#[tokio::test]
async fn responses_parallel_calls_replay_call_ids_and_encrypted_reasoning() {
    let fixture = concat!(
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"encrypted_content\":\"opaque\",\"summary\":[]}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_2\",\"call_id\":\"call_2\",\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"b\\\"}\"}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"a\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
    );
    let (url, captures) = capture_server(fixture).await;
    let provider = OpenAiResponsesProvider::new(url, "test-key");
    let deltas = collect(provider.stream(request()).await.unwrap()).await;
    let calls: Vec<_> = deltas
        .into_iter()
        .filter_map(|delta| match delta {
            ChatDelta::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[1].id, "call_2");
    assert_eq!(calls[0].args, json!({"file_path":"a"}));
    let mut req = request();
    req.messages
        .push(ChatMessage::assistant_tool_calls("", calls));
    req.messages
        .push(ChatMessage::tool_result("call_1", "Read", "first result"));
    req.messages
        .push(ChatMessage::tool_result("call_2", "Read", "second result"));
    collect(provider.stream(req).await.unwrap()).await;
    let captures = captures.lock().await;
    let input = captures[1]["input"].as_array().unwrap();
    assert_eq!(input.len(), 6);
    assert_eq!(
        input[1],
        json!({"type":"reasoning","id":"rs_1","encrypted_content":"opaque","summary":[]})
    );
    assert_eq!(input[2]["call_id"], "call_1");
    assert_eq!(input[3]["call_id"], "call_2");
    assert_eq!(input[4]["output"], "first result");
}

#[tokio::test]
async fn responses_requires_successful_completion_before_releasing_any_calls() {
    for fixture in [
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"server failed\"}}}\n\n",
        "data: {\"type\":\"error\",\"message\":\"quota exceeded\"}\n\n",
        "data: invalid-json\n\n",
        concat!(
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"Read\",\"arguments\":\"{}\"}}\n\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{}}\n\n",
        ),
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"Read\",\"arguments\":\"{}\"},{\"type\":\"function_call\",\"call_id\":\"call_2\",\"name\":\"Read\",\"arguments\":\"{\"}]}}\n\n",
    ] {
        let url = fake_server(fixture).await;
        let deltas = collect(OpenAiResponsesProvider::new(url, "test-key").stream(request()).await.unwrap()).await;
        assert!(matches!(deltas.last(), Some(ChatDelta::Truncated(_))), "{deltas:?}");
        assert!(!deltas.iter().any(|delta| matches!(delta, ChatDelta::Done | ChatDelta::ToolCall(_))));
    }
}

#[tokio::test]
async fn responses_transport_failure_is_truncated() {
    let url = truncating_server(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
    )
    .await;
    let deltas = collect(
        OpenAiResponsesProvider::new(url, "test-key")
            .stream(request())
            .await
            .unwrap(),
    )
    .await;
    assert!(matches!(deltas.last(), Some(ChatDelta::Truncated(_))));
    assert!(!deltas.iter().any(|delta| matches!(delta, ChatDelta::Done)));
}

/// Answer every request with `status` and `body`, keeping each request's
/// headers.
async fn status_server(
    status: u16,
    body: &'static str,
) -> (String, Arc<Mutex<Vec<axum::http::HeaderMap>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let captures = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback({
        let captures = Arc::clone(&captures);
        move |request: axum::http::Request<Body>| {
            let captures = Arc::clone(&captures);
            async move {
                captures.lock().await.push(request.headers().clone());
                let status = axum::http::StatusCode::from_u16(status).expect("status");
                (status, [(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake provider");
    });
    (format!("http://{addr}"), captures)
}

fn configured(engine: Engine, url: String, headers: &[(&str, &str)]) -> Box<dyn LlmProvider> {
    let cfg = ProviderConfig {
        kind: "openai-compatible".to_string(),
        base_url: Some(url),
        api_key: "test-key".to_string(),
        headers: headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        user_agent: Some("qunica-contract".to_string()),
        default_model: "test-model".to_string(),
        reasoning_passback: false,
        supports_temperature: true,
        supports_reasoning_effort: true,
        context_window_tokens: None,
        context_output_reserve_ratio: None,
    };
    match engine {
        Engine::Native => build_provider(&cfg).expect("native provider"),
        #[cfg(feature = "rig-engine")]
        Engine::Rig => {
            qunica_backend::llm::rig_engine::build_rig_provider(&cfg).expect("rig provider")
        }
    }
}

async fn llm_contract_openai_configured_headers_never_replace_the_key(engine: Engine) {
    let (url, captures) = status_server(200, OPENAI_STOP).await;
    let provider = configured(
        engine,
        url,
        &[
            ("Authorization", "Bearer configured"),
            ("X-Gateway-Team", "qunica"),
        ],
    );

    let _ = collect(provider.stream(request()).await.unwrap()).await;

    let captured = captures.lock().await;
    let auth: Vec<_> = captured[0].get_all(header::AUTHORIZATION).iter().collect();
    assert_eq!(auth, ["Bearer test-key"], "one credential, the key's");
    assert_eq!(captured[0]["x-gateway-team"], "qunica");
    assert_eq!(captured[0][header::USER_AGENT], "qunica-contract");
}

async fn llm_contract_openai_keeps_the_error_body_for_classification(engine: Engine) {
    for (status, body, check) in [
        (
            400,
            "{\"error\":{\"code\":\"context_length_exceeded\",\"message\":\"This model's maximum context length is 128000 tokens.\"}}",
            ProviderHttpError::is_context_overflow as fn(&ProviderHttpError) -> bool,
        ),
        (
            400,
            "{\"error\":{\"message\":\"The `reasoning_content` in the thinking mode must be passed back to the API.\"}}",
            ProviderHttpError::requires_reasoning_passback,
        ),
        (429, "{\"error\":{\"message\":\"Rate limit reached\"}}", ProviderHttpError::is_transient),
    ] {
        let (url, _) = status_server(status, body).await;
        let error = openai(engine, url)
            .stream(request())
            .await
            .expect_err("a rejected request fails before streaming");
        let http = error
            .downcast_ref::<ProviderHttpError>()
            .unwrap_or_else(|| panic!("not a ProviderHttpError: {error:#}"));
        assert_eq!(http.status, status);
        assert_eq!(http.body, body);
        assert!(check(http), "{body}");
    }
}

/// A Rig turn travels back as itself: the tool round the runtime stores from
/// the deltas replays with the same call and arguments.
#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_replays_a_stored_tool_round() {
    let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"look first\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_9\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"a.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
                data: [DONE]\n\n";
    let (url, captures) = capture_server(body).await;
    let provider = openai(Engine::Rig, url);

    let deltas = collect(provider.stream(continuation_request()).await.unwrap()).await;
    let native = deltas
        .iter()
        .find_map(|delta| match delta {
            ChatDelta::NativeContent(native) => Some(native.clone()),
            _ => None,
        })
        .expect("the Rig turn is surfaced for storage");
    let calls: Vec<ToolCall> = deltas
        .iter()
        .filter_map(|delta| match delta {
            ChatDelta::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .collect();
    let mut stored = ChatMessage::assistant_tool_calls("", calls).with_reasoning("look first");
    stored.native_content = Some(native);

    let mut next = continuation_request();
    next.reasoning_passback = true;
    next.messages.truncate(2);
    next.messages.push(stored);
    next.messages
        .push(ChatMessage::tool_result("call_9", "Read", "a body"));
    let _ = collect(provider.stream(next).await.unwrap()).await;

    let captured = captures.lock().await;
    let replayed = &captured[1]["messages"][2];
    assert_eq!(replayed["tool_calls"][0]["id"], "call_9");
    assert_eq!(
        replayed["tool_calls"][0]["function"]["arguments"],
        "{\"file_path\":\"a.txt\"}"
    );
    assert_eq!(replayed["reasoning_content"], "look first");
    assert_eq!(captured[1]["messages"][3]["tool_call_id"], "call_9");
}

on_every_engine!(
    llm_contract_openai_configured_headers_never_replace_the_key,
    llm_contract_openai_keeps_the_error_body_for_classification,
    llm_contract_openai_maps_content_to_tokens,
    llm_contract_openai_maps_reasoning_content_and_reasoning_to_reasoning,
    llm_contract_openai_maps_usage_and_streamed_tool_calls,
    llm_contract_openai_emits_tool_calls_finished_with_stop,
    llm_contract_openai_drops_tool_calls_truncated_by_the_token_limit,
    llm_contract_openai_reports_a_cut_stream_as_truncated_not_done,
    llm_contract_openai_serializes_tool_continuation_messages,
    llm_contract_openai_sends_reasoning_back_only_when_the_model_asks_for_it,
    llm_contract_openai_omits_blank_reasoning_even_with_passback_on,
    llm_contract_openai_omits_absent_temperature,
    llm_contract_openai_serializes_sampling_as_f64,
    llm_contract_openai_serializes_image_parts_and_preserves_text_only_shape,
    llm_contract_openai_serializes_system_image_parts,
    llm_contract_openai_maps_effort_to_its_own_enum,
    llm_contract_openai_never_sends_a_thinking_budget,
);
