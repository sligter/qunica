//! Provider contract tests.
//!
//! Each test spins up a local HTTP server that replays a canned server-sent
//! event body, points a provider at it, and asserts that the provider maps the
//! provider-specific wire format onto the neutral [`ChatDelta`] vocabulary.
//! No live external API is contacted.

use std::sync::Arc;

use qunica_backend::llm::{
    AnthropicProvider, ChatDelta, ChatMessage, ChatRequest, GeminiProvider, LlmProvider,
    OpenAiCompatibleProvider, ReasoningEffort, ToolCall,
};
use qunica_domain::runtime::ChatContentPart;
use axum::{body::Body, http::header, response::IntoResponse, Router};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;

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
        reasoning_passback: false,
        include_empty_tools: false,
        tools: Vec::new(),
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

#[tokio::test]
async fn llm_contract_openai_maps_content_to_tokens() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\
                data: [DONE]\n";
    let url = fake_server(body).await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hello", " world"]);
    assert!(ends_with_done(&deltas));
}

#[tokio::test]
async fn llm_contract_openai_maps_reasoning_content_and_reasoning_to_reasoning() {
    // `reasoning_content` (DeepSeek-style) and `reasoning` (OpenAI-style) both
    // map to ChatDelta::Reasoning.
    let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"step 1\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"reasoning\":\"step 2\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\
                data: [DONE]\n";
    let url = fake_server(body).await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(reasoning(&deltas), vec!["step 1", "step 2"]);
    assert_eq!(tokens(&deltas), vec!["answer"]);
}

#[tokio::test]
async fn llm_contract_openai_maps_usage_and_streamed_tool_calls() {
    let body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Paris\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\
                data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\
                data: [DONE]\n";
    let url = fake_server(body).await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call_1");
    assert_eq!(calls[0].1, "get_weather");
    assert_eq!(calls[0].2, serde_json::json!({ "city": "Paris" }));

    assert_eq!(usages(&deltas), vec![(Some(12), Some(7), Some(19))]);
}

#[tokio::test]
async fn llm_contract_openai_emits_tool_calls_finished_with_stop() {
    // Some OpenAI-compatible gateways close a tool-calling turn with `stop`
    // instead of `tool_calls`. The buffered call still has to be emitted, or the
    // agent's round ends with no tool result and no text.
    let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\"}\"}}]},\"finish_reason\":\"stop\"}]}\n\
                data: [DONE]\n";
    let url = fake_server(body).await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    let calls = tool_calls(&deltas);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "Read");
    assert_eq!(calls[0].2, serde_json::json!({ "file_path": "note.txt" }));
}

#[tokio::test]
async fn llm_contract_openai_drops_tool_calls_truncated_by_the_token_limit() {
    // `length` means the arguments were cut mid-JSON, so the call is incomplete
    // and must not be handed to the tool executor.
    let body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\"}}]},\"finish_reason\":\"length\"}]}\n\
                data: [DONE]\n";
    let url = fake_server(body).await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert!(tool_calls(&deltas).is_empty());
}

#[tokio::test]
async fn llm_contract_openai_reports_a_cut_stream_as_truncated_not_done() {
    // The provider hung up mid-body. The deltas that did arrive are kept, but
    // the stream must close with `Truncated` — ending it with `Done` would make
    // a dropped connection indistinguishable from a model that stopped talking.
    let url = truncating_server("data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let deltas = collect(provider.stream(request()).await.unwrap()).await;

    assert_eq!(tokens(&deltas), vec!["Hel"]);
    assert!(matches!(deltas.last(), Some(ChatDelta::Truncated(_))));
    assert!(!ends_with_done(&deltas));
}

#[tokio::test]
async fn llm_contract_openai_serializes_tool_continuation_messages() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

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
    assert_eq!(messages[3]["name"], "Read");
    assert_eq!(messages[3]["content"], "file body");
}

#[tokio::test]
async fn llm_contract_openai_sends_reasoning_back_only_when_the_model_asks_for_it() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let mut off = continuation_request();
    off.messages[2] = off.messages[2]
        .clone()
        .with_reasoning("weighing the options");
    let _ = collect(provider.stream(off.clone()).await.unwrap()).await;

    let mut on = off;
    on.reasoning_passback = true;
    let _ = collect(provider.stream(on).await.unwrap()).await;

    let captured = captures.lock().await;
    // Off is the default because DeepSeek's own `deepseek-reasoner` rejects a
    // request that echoes reasoning back.
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

#[tokio::test]
async fn llm_contract_openai_omits_blank_reasoning_even_with_passback_on() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");
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

#[tokio::test]
async fn llm_contract_openai_omits_absent_temperature() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");
    let mut request = request();
    request.temperature = None;

    let _ = collect(provider.stream(request).await.unwrap()).await;

    let captured = captures.lock().await;
    assert!(captured[0].get("temperature").is_none(), "{}", captured[0]);
}

#[tokio::test]
async fn llm_contract_openai_serializes_image_parts_and_preserves_text_only_shape() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    let _ = collect(provider.stream(image_request("user")).await.unwrap()).await;
    let _ = collect(provider.stream(request()).await.unwrap()).await;

    let captured = captures.lock().await;
    assert_eq!(
        captured[0]["messages"][0]["content"],
        json!([
            { "type": "text", "text": "describe this image" },
            {
                "type": "image_url",
                "image_url": { "url": "data:image/png;base64,AQID" }
            }
        ])
    );
    assert_eq!(captured[1]["messages"][0]["content"], "hi");
}

#[tokio::test]
async fn llm_contract_openai_serializes_system_image_parts() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

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

#[tokio::test]
async fn llm_contract_openai_maps_effort_to_its_own_enum() {
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    for (effort, expected) in [
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "xhigh"),
        (ReasoningEffort::Max, "max"),
    ] {
        let body = captured_body(&provider, &captures, Some(effort)).await;
        assert_eq!(body["reasoning_effort"], json!(expected));
    }
}

#[tokio::test]
async fn llm_contract_openai_never_sends_a_thinking_budget() {
    // This endpoint's vocabulary for thinking is the level. A budget key is an
    // unknown parameter here — at best ignored, at worst a rejected request.
    let (url, captures) = capture_server("data: [DONE]\n").await;
    let provider = OpenAiCompatibleProvider::new(url, "test-key");

    for effort in ReasoningEffort::ALL {
        let body = captured_body(&provider, &captures, Some(effort)).await;
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
