//! Provider contract tests.
//!
//! Each test spins up a local HTTP server that replays a canned server-sent
//! event body, points a provider at it, and asserts that the provider maps the
//! provider-specific wire format onto the neutral [`ChatDelta`] vocabulary.
//! No live external API is contacted.

use std::sync::Arc;

use ag_swarmer_backend::llm::{
    AnthropicProvider, ChatDelta, ChatMessage, ChatRequest, GeminiProvider, LlmProvider,
    OpenAiCompatibleProvider, ToolCall,
};
use axum::{body::Body, http::header, response::IntoResponse, Router};
use serde_json::{json, Value};
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
    assert_eq!(
        usages(&deltas),
        vec![(Some(25), None, None), (None, Some(9), None)]
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
