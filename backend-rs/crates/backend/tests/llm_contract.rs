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

/// The engines a contract test runs against: every supported provider case
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

fn anthropic(engine: Engine, url: String) -> Box<dyn LlmProvider> {
    match engine {
        Engine::Native => Box::new(AnthropicProvider::new(url, "test-key")),
        #[cfg(feature = "rig-engine")]
        Engine::Rig => Box::new(
            qunica_backend::llm::rig_engine::RigProvider::anthropic(url, "test-key").unwrap(),
        ),
    }
}

fn gemini(engine: Engine, url: String) -> Box<dyn LlmProvider> {
    match engine {
        Engine::Native => Box::new(GeminiProvider::new(url, "test-key")),
        #[cfg(feature = "rig-engine")]
        Engine::Rig => {
            Box::new(qunica_backend::llm::rig_engine::RigProvider::gemini(url, "test-key").unwrap())
        }
    }
}

const ANTHROPIC_STOP: &str = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
const GEMINI_STOP: &str = "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n\n";

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
        (
            [
                (header::CONTENT_TYPE, "text/event-stream"),
                (header::CONNECTION, "close"),
            ],
            body,
        )
            .into_response()
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
                (
                    [
                        (header::CONTENT_TYPE, "text/event-stream"),
                        (header::CONNECTION, "close"),
                    ],
                    body,
                )
                    .into_response()
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
        tools: vec![ToolDefinition {
            name: "Read".into(),
            description: "test".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        }],
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
                        "thoughtSignature": "c2lnLTEyMw=="
                    })),
                }],
            ),
            ChatMessage::tool_result("search", "search", "{\"ok\":true}"),
        ],
        temperature: Some(0.0),
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: vec![ToolDefinition {
            name: "search".into(),
            description: "test".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        }],
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

async fn llm_contract_anthropic_maps_text_and_thinking_deltas(engine: Engine) {
    let body = "event: message_start\n\
                data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25}}}\n\n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"pondering\"}}\n\n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
                event: message_delta\n\
                data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":9}}\n\n\
                event: message_stop\n\
                data: {\"type\":\"message_stop\"}\n\n";
    let url = fake_server(body).await;
    let provider = anthropic(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hello"]);
    assert_eq!(reasoning(&deltas), vec!["pondering"]);
    // Anthropic reports the input count once in `message_start` and only the
    // output count in `message_delta`, so the closing usage carries the
    // remembered input forward to report a turn total.
    assert_eq!(
        usages(&deltas),
        if engine.is_rig() {
            vec![(Some(25), Some(9), Some(34))]
        } else {
            vec![(Some(25), None, None), (Some(25), Some(9), Some(34))]
        }
    );
    assert!(ends_with_done(&deltas));
}

async fn llm_contract_anthropic_maps_streamed_tool_use_block(engine: Engine) {
    let body = "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"lookup\"}}\n\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\"}}\n\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"rust\\\"}\"}}\n\n\
                data: {\"type\":\"content_block_stop\",\"index\":0}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n";
    let url = fake_server(body).await;
    let provider = anthropic(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "toolu_1");
    assert_eq!(calls[0].1, "lookup");
    assert_eq!(calls[0].2, serde_json::json!({ "q": "rust" }));
}

async fn llm_contract_anthropic_serializes_system_and_tool_continuation_messages(engine: Engine) {
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

    let _ = collect(provider.stream(continuation_request()).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(
        if engine.is_rig() {
            &captured[0]["system"][0]["text"]
        } else {
            &captured[0]["system"]
        },
        "Use tools carefully."
    );
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

async fn llm_contract_anthropic_serializes_image_parts_and_preserves_text_only_shape(
    engine: Engine,
) {
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

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
    assert_eq!(
        if engine.is_rig() {
            &captured[1]["messages"][0]["content"][0]["text"]
        } else {
            &captured[1]["messages"][0]["content"]
        },
        "hi"
    );
}

async fn llm_contract_anthropic_serializes_system_image_parts(engine: Engine) {
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

    if engine.is_rig() {
        assert!(provider.stream(image_request("system")).await.is_err());
        assert!(captures.lock().await.is_empty());
        return;
    }
    let _ = collect(provider.stream(image_request("system")).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(captured[0]["system"][0]["type"], "text");
    assert_eq!(captured[0]["system"][1]["source"]["type"], "base64");
    assert_eq!(captured[0]["system"][1]["source"]["data"], "AQID");
}

async fn llm_contract_anthropic_coalesces_parallel_tool_results(engine: Engine) {
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

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
    assert_eq!(
        if engine.is_rig() {
            &content[0]["content"][0]["text"]
        } else {
            &content[0]["content"]
        },
        "a body"
    );
    assert_eq!(content[1]["type"], "tool_result");
    assert_eq!(content[1]["tool_use_id"], "call_2");
    assert_eq!(
        if engine.is_rig() {
            &content[1]["content"][0]["text"]
        } else {
            &content[1]["content"]
        },
        "b body"
    );
}

async fn llm_contract_gemini_maps_text_and_usage(engine: Engine) {
    let body = "data: {\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}\n\n\
                data: {\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\" there\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":3,\"totalTokenCount\":8}}\n\n";
    let url = fake_server(body).await;
    let provider = gemini(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hello", " there"]);
    assert_eq!(usages(&deltas), vec![(Some(5), Some(3), Some(8))]);
    assert!(ends_with_done(&deltas));
}

async fn llm_contract_gemini_maps_function_call(engine: Engine) {
    let body = "data: {\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"search\",\"args\":{\"q\":\"rust\"}}}]}}]}\n\n";
    let url = fake_server(body).await;
    let provider = gemini(engine, url);

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "search");
    assert_eq!(calls[0].2, serde_json::json!({ "q": "rust" }));
}

async fn llm_contract_gemini_preserves_function_call_thought_signature(engine: Engine) {
    let body = "data: {\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"thoughtSignature\":\"c2lnLTEyMw==\",\"functionCall\":{\"name\":\"search\",\"args\":{\"q\":\"rust\"}}}]}}]}\n\n";
    let url = fake_server(body).await;
    let provider = gemini(engine, url);

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
        Some("c2lnLTEyMw==")
    );
}

async fn llm_contract_gemini_serializes_function_response_continuation_messages(engine: Engine) {
    let (url, captures) = capture_server(GEMINI_STOP).await;
    let provider = gemini(engine, url);

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

async fn llm_contract_gemini_serializes_image_parts_and_preserves_text_only_shape(engine: Engine) {
    let (url, captures) = capture_server(GEMINI_STOP).await;
    let provider = gemini(engine, url);

    let _ = collect(provider.stream(image_request("user")).await.unwrap()).await;
    let _ = collect(provider.stream(request()).await.unwrap()).await;

    let mut captured = captures.lock().await.clone();
    if !engine.is_rig() {
        for body in &mut captured {
            for part in body["contents"][0]["parts"].as_array_mut().unwrap() {
                part["thought"] = json!(false);
            }
        }
    }
    assert_eq!(
        captured[0]["contents"][0]["parts"],
        json!([
            { "text": "describe this image", "thought": false },
            { "inlineData": { "mimeType": "image/png", "data": "AQID" }, "thought": false }
        ])
    );
    assert_eq!(
        captured[1]["contents"][0]["parts"],
        json!([{ "text": "hi", "thought": false }])
    );
}

async fn llm_contract_gemini_serializes_system_image_parts(engine: Engine) {
    let (url, captures) = capture_server(GEMINI_STOP).await;
    let provider = gemini(engine, url);

    if engine.is_rig() {
        assert!(provider.stream(image_request("system")).await.is_err());
        assert!(captures.lock().await.is_empty());
        return;
    }
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

async fn llm_contract_gemini_replays_function_call_thought_signature(engine: Engine) {
    let (url, captures) = capture_server(GEMINI_STOP).await;
    let provider = gemini(engine, url);

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
    assert_eq!(contents[1]["parts"][0]["thoughtSignature"], "c2lnLTEyMw==");
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

async fn llm_contract_anthropic_maps_effort_to_a_thinking_budget(engine: Engine) {
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

    let mut previous = 0;
    for effort in ReasoningEffort::ALL {
        let body = captured_body(provider.as_ref(), &captures, Some(effort)).await;
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

async fn llm_contract_anthropic_drops_temperature_while_thinking(engine: Engine) {
    // Extended thinking fixes sampling: Anthropic rejects any temperature but 1
    // while it is on, and `effort_request` asks for 0.0.
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

    let body = captured_body(provider.as_ref(), &captures, Some(ReasoningEffort::High)).await;
    assert!(body.get("temperature").is_none(), "{body}");

    // Without thinking the setting applies as asked — and is absent rather
    // than null when there is none.
    let body = captured_body(provider.as_ref(), &captures, None).await;
    assert_eq!(body["temperature"], json!(0.0));
}

async fn llm_contract_anthropic_sends_signed_thinking_back_with_its_tool_calls(engine: Engine) {
    // With thinking on, Anthropic requires the assistant turn that made the
    // tool calls to lead with the signed thinking block that produced them;
    // tool results following a bare text block are rejected outright.
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

    let mut request = effort_request(Some(ReasoningEffort::High));
    request.tools = gemini_thought_signature_request().tools;
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

async fn llm_contract_anthropic_omits_thinking_it_cannot_prove(engine: Engine) {
    // Reasoning replayed from stored history has no signature, and the
    // provider verifies one against the other — so an unsigned block is a
    // rejected request rather than a preserved thought. The same block is
    // equally invalid when thinking is off.
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let provider = anthropic(engine, url);

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
    unsigned.tools = gemini_thought_signature_request().tools;
    unsigned.messages = vec![ChatMessage::text("user", "search"), assistant.clone()];
    let _ = collect(provider.stream(unsigned).await.unwrap()).await;

    let mut thinking_off = effort_request(None);
    thinking_off.tools = gemini_thought_signature_request().tools;
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

async fn llm_contract_anthropic_surfaces_the_signature_over_its_thinking(engine: Engine) {
    let url = fake_server(concat!(
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"step\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-abc\"}}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
    ))
    .await;
    let provider = anthropic(engine, url);

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

async fn llm_contract_anthropic_reports_a_mid_stream_error_as_truncation(engine: Engine) {
    // The status was 200 and the body began normally, so nothing else marks
    // this as a failure: ending on `Done` would pass a dropped answer off as a
    // model that chose to say nothing.
    let url = fake_server(concat!(
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    ))
    .await;
    let provider = anthropic(engine, url);

    let deltas = collect(provider.stream(effort_request(None)).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["partial".to_string()]);
    assert!(
        deltas.iter().any(
            |delta| matches!(delta, ChatDelta::Truncated(reason) if reason.contains("Overloaded"))
        ),
        "{deltas:?}"
    );
}

async fn llm_contract_gemini_maps_effort_to_a_thinking_config(engine: Engine) {
    let (url, captures) = capture_server(GEMINI_STOP).await;
    let provider = gemini(engine, url);
    for effort in ReasoningEffort::ALL {
        let mut req = effort_request(Some(effort));
        req.top_p = Some(0.7);
        assert!(ends_with_done(
            &collect(provider.stream(req).await.unwrap()).await
        ));
        let body = captures.lock().await.last().unwrap().clone();
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            effort.thinking_budget_tokens()
        );
        assert_eq!(body["generationConfig"]["topP"], 0.7);
        assert_eq!(body["generationConfig"]["temperature"], 0.0);
    }
}

#[tokio::test]
async fn llm_contract_absent_effort_omits_the_key_entirely() {
    for &engine in ENGINES {
        for (kind, stop, key) in [
            ("openai-compatible", OPENAI_STOP, "/reasoning_effort"),
            ("anthropic", ANTHROPIC_STOP, "/thinking"),
            ("gemini", GEMINI_STOP, "/generationConfig/thinkingConfig"),
        ] {
            let (url, captures) = capture_server(stop).await;
            let provider = configured_kind(engine, kind, url, &[]);
            let body = captured_body(provider.as_ref(), &captures, None).await;
            assert!(body.pointer(key).is_none(), "{kind}: {body}");
        }
    }
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
                (
                    status,
                    [
                        (header::CONTENT_TYPE, "text/event-stream"),
                        (header::CONNECTION, "close"),
                    ],
                    body,
                )
                    .into_response()
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
    configured_kind(engine, "openai-compatible", url, headers)
}

fn configured_kind(
    engine: Engine,
    kind: &str,
    url: String,
    headers: &[(&str, &str)],
) -> Box<dyn LlmProvider> {
    let cfg = ProviderConfig {
        kind: kind.to_string(),
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
    llm_contract_anthropic_maps_text_and_thinking_deltas,
    llm_contract_anthropic_maps_streamed_tool_use_block,
    llm_contract_anthropic_serializes_system_and_tool_continuation_messages,
    llm_contract_anthropic_serializes_image_parts_and_preserves_text_only_shape,
    llm_contract_anthropic_serializes_system_image_parts,
    llm_contract_anthropic_coalesces_parallel_tool_results,
    llm_contract_anthropic_maps_effort_to_a_thinking_budget,
    llm_contract_anthropic_drops_temperature_while_thinking,
    llm_contract_anthropic_sends_signed_thinking_back_with_its_tool_calls,
    llm_contract_anthropic_omits_thinking_it_cannot_prove,
    llm_contract_anthropic_surfaces_the_signature_over_its_thinking,
    llm_contract_anthropic_reports_a_mid_stream_error_as_truncation,
    llm_contract_gemini_maps_text_and_usage,
    llm_contract_gemini_maps_function_call,
    llm_contract_gemini_preserves_function_call_thought_signature,
    llm_contract_gemini_serializes_function_response_continuation_messages,
    llm_contract_gemini_serializes_image_parts_and_preserves_text_only_shape,
    llm_contract_gemini_serializes_system_image_parts,
    llm_contract_gemini_replays_function_call_thought_signature,
    llm_contract_gemini_maps_effort_to_a_thinking_config,
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

#[tokio::test]
async fn llm_contract_vendor_configured_transport_and_errors() {
    for &engine in ENGINES {
        for (kind, stop) in [("anthropic", ANTHROPIC_STOP), ("google", GEMINI_STOP)] {
            let (url, headers) = status_server(200, stop).await;
            let provider = configured_kind(
                engine,
                kind,
                url,
                &[("x-api-key", "configured"), ("x-gateway-team", "qunica")],
            );
            assert!(ends_with_done(
                &collect(provider.stream(request()).await.unwrap()).await
            ));
            let headers = headers.lock().await;
            if kind == "anthropic" {
                assert_eq!(
                    headers[0].get_all("x-api-key").iter().collect::<Vec<_>>(),
                    ["test-key"]
                );
                assert_eq!(headers[0]["anthropic-version"], "2023-06-01");
            }
            assert_eq!(headers[0]["x-gateway-team"], "qunica");
            assert_eq!(headers[0][header::USER_AGENT], "qunica-contract");
            for (status, body) in [
                (400, "{\"error\":{\"message\":\"prompt is too long\"}}"),
                (429, "{\"error\":{\"message\":\"rate limit\"}}"),
                (503, "{\"error\":{\"message\":\"unavailable\"}}"),
            ] {
                let (url, _) = status_server(status, body).await;
                let error = configured_kind(engine, kind, url, &[])
                    .stream(request())
                    .await
                    .expect_err("HTTP rejection");
                let http = error
                    .downcast_ref::<ProviderHttpError>()
                    .unwrap_or_else(|| panic!("{error:#}"));
                assert_eq!(http.status, status);
                assert_eq!(http.body, body);
                assert_eq!(http.is_context_overflow(), status == 400);
                assert_eq!(http.is_transient(), status != 400);
            }
        }
    }
}

#[cfg(feature = "rig-engine")]
fn stored_rig_message(deltas: &[ChatDelta]) -> ChatMessage {
    let mut message = ChatMessage::assistant_tool_calls(
        tokens(deltas).concat(),
        deltas
            .iter()
            .filter_map(|d| {
                if let ChatDelta::ToolCall(call) = d {
                    Some(call.clone())
                } else {
                    None
                }
            })
            .collect(),
    )
    .with_reasoning(reasoning(deltas).concat());
    for delta in deltas {
        match delta {
            ChatDelta::ReasoningSignature(sig) => message.reasoning_signature = Some(sig.clone()),
            ChatDelta::NativeContent(native) => message.native_content = Some(native.clone()),
            _ => {}
        }
    }
    assert!(message.native_content.is_some());
    serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap()
}

#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_anthropic_replays_persisted_signed_blocks() {
    const BODY: &str = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":25,\"cache_creation_input_tokens\":5}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"first\",\"signature\":\"sig-1\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"opaque\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"second\",\"signature\":\"sig-2\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"search\",\"input\":{\"q\":\"rust\"}}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n"
    );
    for model in ["test-model", "claude-opus-5"] {
        let (url, captures) = capture_server(BODY).await;
        let provider = anthropic(Engine::Rig, url);
        let mut req = effort_request(Some(ReasoningEffort::High));
        req.model = model.into();
        req.tools = gemini_thought_signature_request().tools;
        let deltas = collect(provider.stream(req.clone()).await.unwrap()).await;
        assert!(ends_with_done(&deltas), "{deltas:?}");
        let stored = stored_rig_message(&deltas);
        assert_eq!(stored.reasoning_content.as_deref(), Some("firstsecond"));
        assert_eq!(stored.reasoning_signature.as_deref(), Some("sig-2"));
        let usage = deltas
            .iter()
            .find_map(|d| {
                if let ChatDelta::Usage(u) = d {
                    Some(u)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            (
                usage.input_tokens,
                usage.cached_input_tokens,
                usage.total_tokens
            ),
            (Some(40), Some(25), Some(42))
        );
        req.messages.push(stored.clone());
        req.messages
            .push(ChatMessage::tool_result("toolu_1", "search", "found"));
        assert!(ends_with_done(
            &collect(provider.stream(req.clone()).await.unwrap()).await
        ));
        let blocks = captures.lock().await[1]["messages"][1]["content"].clone();
        assert_eq!(
            blocks,
            json!([
                {"type": "thinking", "thinking": "first", "signature": "sig-1"},
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "thinking", "thinking": "second", "signature": "sig-2"},
                {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"q": "rust"}}
            ])
        );
        if model == "test-model" {
            // P0 native checkpoints import the same full ordered blocks, too.
            req.messages[1].native_content = Some(json!({"anthropic": blocks.clone()}));
            assert!(ends_with_done(
                &collect(provider.stream(req).await.unwrap()).await
            ));
            assert_eq!(captures.lock().await[2]["messages"][1]["content"], blocks);
        }
    }
}

#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_gemini_replays_persisted_thought_signature() {
    const BODY: &str = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"plan\",\"thought\":true},{\"functionCall\":{\"name\":\"search\",\"args\":{\"q\":\"rust\"}},\"thoughtSignature\":\"c2lnLW9yaWdpbmFs\",\"partMetadata\":{\"opaque\":7}}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":10,\"cachedContentTokenCount\":6,\"candidatesTokenCount\":3,\"totalTokenCount\":15}}\n\n";
    let (url, captures) = capture_server(BODY).await;
    let provider = gemini(Engine::Rig, url);
    let mut req = request();
    req.model = "gemini-2.5-pro".into();
    req.tools = gemini_thought_signature_request().tools;
    let deltas = collect(provider.stream(req.clone()).await.unwrap()).await;
    assert!(ends_with_done(&deltas), "{deltas:?}");
    assert_eq!(reasoning(&deltas), ["plan"]);
    let stored = stored_rig_message(&deltas);
    assert_eq!(
        stored.tool_calls[0].provider_metadata.as_ref().unwrap()["thoughtSignature"],
        "c2lnLW9yaWdpbmFs"
    );
    let id = stored.tool_calls[0].id.clone();
    req.messages.push(stored);
    req.messages
        .push(ChatMessage::tool_result(id, "search", "found"));
    assert!(ends_with_done(
        &collect(provider.stream(req).await.unwrap()).await
    ));
    assert_eq!(
        captures.lock().await[1]["contents"][1]["parts"][1],
        json!({"functionCall": {"name": "search", "args": {"q": "rust"}}, "thoughtSignature": "c2lnLW9yaWdpbmFs", "partMetadata": {"opaque": 7}})
    );
}

#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_vendors_never_release_unaccepted_tool_calls() {
    for (kind, partial) in [
        ("anthropic", "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"search\",\"input\":{}}}\n\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n"),
        ("gemini", "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"search\",\"args\":{}}}]}}]}\n\n"),
    ] {
        for cut_transport in [false, true] {
            let url = if cut_transport { truncating_server(partial).await } else { fake_server(partial).await };
            match configured_kind(Engine::Rig, kind, url, &[]).stream(request()).await {
                Err(_) => {}, // Failure before any displayable item is equally safe.
                Ok(rx) => {
                    let deltas = collect(rx).await;
                    assert!(tool_calls(&deltas).is_empty(), "{kind}: {deltas:?}");
                    assert!(!ends_with_done(&deltas), "{kind}: {deltas:?}");
                    assert!(deltas.iter().any(|d| matches!(d, ChatDelta::Truncated(_))), "{kind}: {deltas:?}");
                }
            }
        }
    }
}

#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_gemini_refuses_malformed_signatures_before_io() {
    let (url, captures) = capture_server(GEMINI_STOP).await;
    let provider = gemini(Engine::Rig, url);
    let mut req = gemini_thought_signature_request();
    req.messages[1].tool_calls[0].provider_metadata =
        Some(json!({"thoughtSignature": "bad-signature"}));
    assert!(provider
        .stream(req)
        .await
        .unwrap_err()
        .to_string()
        .contains("Base64"));
    assert!(captures.lock().await.is_empty());
}

#[tokio::test]
async fn llm_contract_vendors_preserve_explicit_empty_tools() {
    for &engine in ENGINES {
        for (kind, stop) in [
            ("anthropic-compatible", ANTHROPIC_STOP),
            ("gemini", GEMINI_STOP),
        ] {
            let (url, captures) = capture_server(stop).await;
            let provider = configured_kind(engine, kind, url, &[]);
            let mut req = request();
            req.include_empty_tools = true;
            assert!(ends_with_done(
                &collect(provider.stream(req).await.unwrap()).await
            ));
            assert_eq!(captures.lock().await[0]["tools"], json!([]));
        }
    }
}

#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_vendors_reject_malformed_tool_arguments() {
    for (kind, body) in [
        ("anthropic", "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"search\",\"input\":{}}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{bad\"}}\n\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n"),
        ("gemini", "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"search\",\"args\":\"bad\"}}]},\"finishReason\":\"STOP\"}]}\n\n"),
    ] {
        let provider = configured_kind(Engine::Rig, kind, fake_server(body).await, &[]);
        if let Ok(rx) = provider.stream(request()).await {
            let deltas = collect(rx).await;
            assert!(tool_calls(&deltas).is_empty(), "{kind}: {deltas:?}");
            assert!(!ends_with_done(&deltas));
            assert!(!deltas.iter().any(|d| matches!(d, ChatDelta::NativeContent(_))));
            if kind == "anthropic" { assert_eq!(usages(&deltas), [(Some(10), Some(2), Some(12))]); }
            assert!(deltas.iter().any(|d| matches!(d, ChatDelta::Truncated(_))), "{kind}: {deltas:?}");
        }
    }
}

#[cfg(feature = "rig-engine")]
#[tokio::test]
async fn llm_contract_rig_refuses_signature_loss_after_model_or_context_change() {
    use qunica_backend::llm::rig_engine::RigMessageAdapter;
    use rig_core::completion::{AssistantContent, AssistantMessage};
    use rig_core::message::{Api, Origin, Reasoning, ToolName};
    for kind in ["anthropic", "gemini"] {
        let block = if kind == "anthropic" {
            AssistantContent::Reasoning(Reasoning::new("signed reasoning")).with_native(
                json!({"type": "thinking", "thinking": "signed reasoning", "signature": "sig"}),
            )
        } else {
            AssistantContent::tool_call("call_1", ToolName::new("Read").unwrap(), json!({"file_path": "note.txt"}))
                .with_native(json!({"functionCall": {"name": "Read", "args": {"file_path": "note.txt"}}, "thoughtSignature": "c2ln"}))
        };
        let origin = if kind == "anthropic" {
            Origin::new(
                Api::from_static("anthropic.messages"),
                "anthropic",
                "old-model",
            )
        } else {
            Origin::new(
                Api::from_static("gemini.generate_content"),
                "gcp.gemini",
                "old-model",
            )
        };
        let turn = AssistantMessage::new(vec![block]).with_origin(origin);
        let mut req = continuation_request();
        req.messages[2] = RigMessageAdapter::assistant_from_rig(&turn);
        if kind == "anthropic" {
            req.messages.pop();
            req.reasoning_effort = Some(ReasoningEffort::High);
        }
        let (url, captures) = capture_server(if kind == "anthropic" {
            ANTHROPIC_STOP
        } else {
            GEMINI_STOP
        })
        .await;
        let error = configured_kind(Engine::Rig, kind, url, &[])
            .stream(req)
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("signed provider history"),
            "{error:#}"
        );
        assert!(captures.lock().await.is_empty());
    }
    // Legacy native checkpoints cannot prove the original context fingerprint
    // for models whose signatures bind the tools/system prompt.
    let (url, captures) = capture_server(ANTHROPIC_STOP).await;
    let mut req = continuation_request();
    req.model = "claude-opus-5".into();
    req.reasoning_effort = Some(ReasoningEffort::High);
    req.messages[2] = req.messages[2]
        .clone()
        .with_reasoning("signed")
        .with_reasoning_signature(Some("sig".into()));
    let error = anthropic(Engine::Rig, url).stream(req).await.unwrap_err();
    assert!(
        format!("{error:#}").contains("signed provider history"),
        "{error:#}"
    );
    assert!(captures.lock().await.is_empty());
}
