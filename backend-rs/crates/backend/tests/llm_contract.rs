//! Provider contract tests.
//!
//! Each test spins up a local HTTP server that replays a canned server-sent
//! event body, points a provider at it, and asserts that the provider maps the
//! provider-specific wire format onto the neutral [`ChatDelta`] vocabulary.
//! No live external API is contacted.

use ag_swarmer_backend::llm::{
    AnthropicProvider, ChatDelta, ChatMessage, ChatRequest, GeminiProvider, LlmProvider,
    OpenAiCompatibleProvider,
};
use axum::{http::header, response::IntoResponse, Router};
use tokio::sync::mpsc::Receiver;

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

/// A minimal request; the canned response is independent of these fields.
fn request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
        }],
        temperature: Some(0.0),
        reasoning_passback: false,
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
