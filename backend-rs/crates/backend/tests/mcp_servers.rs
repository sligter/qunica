//! MCP server configuration API and end-to-end transport tests.
//!
//! The stdio tests spawn a real child process — this very integration test
//! binary, re-invoked with `MCP_FAKE_SERVER` set so the
//! `mcp_servers_fake_stdio_entrypoint` test speaks the MCP stdio protocol
//! instead of running assertions. The SSE and streamable-HTTP tests bind a real
//! local axum server that implements the same tool surface over each wire
//! format. No Node, no Python, no live network.

use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, Mutex as StdMutex},
};

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream;
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::mpsc::{unbounded_channel, UnboundedSender},
};
use tower::ServiceExt;

use qunica_backend::mcp::{
    config::McpServerConfig, McpClient, McpError, McpManager, McpTransportKind,
};

// ---------------------------------------------------------------------------
// HTTP test helpers
// ---------------------------------------------------------------------------

async fn app() -> Router {
    qunica_backend::api::router_for_tests().await
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn authed_json(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": email, "password": "supersecret", "name": "Tester"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, token) = send(
        app,
        post_json(
            "/api/v2/auth/login",
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    token["access_token"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// CRUD API tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_servers_crud_round_trips_every_transport_field() {
    let app = app().await;
    let token = register_and_login(&app, "mcp-crud@example.test").await;

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({
                "name": "GitHub MCP",
                "description": "Issues and PRs",
                "transport": "stdio",
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-github"],
                "env": {"GITHUB_TOKEN": "ghp_secret"},
                "timeout_seconds": 45,
                "tool_filter": ["create_issue"],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["transport"], "stdio");
    assert_eq!(created["command"], "npx");
    assert_eq!(created["args"][1], "@modelcontextprotocol/server-github");
    assert_eq!(created["env"]["GITHUB_TOKEN"], "ghp_secret");
    assert_eq!(created["timeout_seconds"], 45);
    assert_eq!(created["tool_filter"][0], "create_issue");
    assert_eq!(created["enabled"], true);
    // The slug tells the operator exactly how the tools will be named.
    assert_eq!(created["slug"], "github_mcp");

    let server_id = created["id"].as_str().unwrap().to_string();

    let (status, listed) = send(&app, authed("GET", "/api/v2/mcp-servers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // Switch the same row to a streamable-HTTP server.
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &token,
            json!({
                "transport": "streamable-http",
                "command": null,
                "url": "https://mcp.example.test/mcp",
                "headers": {"Authorization": "Bearer sk-abcdefgh"},
                "enabled": false,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["transport"], "streamable-http");
    assert_eq!(updated["url"], "https://mcp.example.test/mcp");
    assert_eq!(updated["enabled"], false);
    // Header values carry bearer tokens and must never come back in full.
    assert_eq!(updated["headers_masked"]["Authorization"], "****efgh");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, listed) = send(&app, authed("GET", "/api/v2/mcp-servers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(listed.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn mcp_servers_reject_configurations_their_transport_cannot_use() {
    let app = app().await;
    let token = register_and_login(&app, "mcp-validate@example.test").await;

    // stdio with no command.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "No command", "transport": "stdio"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // HTTP with a URL that has no scheme.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Bad url", "transport": "sse", "url": "example.test/sse"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // An unsupported transport.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Pigeon", "transport": "carrier-pigeon", "url": "https://a.test"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // A timeout outside the supported range.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Slow", "transport": "stdio", "command": "x", "timeout_seconds": 99999}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn mcp_servers_are_scoped_to_their_owner() {
    let app = app().await;
    let owner = register_and_login(&app, "mcp-owner@example.test").await;
    let stranger = register_and_login(&app, "mcp-stranger@example.test").await;

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &owner,
            json!({"name": "Private", "transport": "stdio", "command": "node"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let server_id = created["id"].as_str().unwrap().to_string();

    let (status, _) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &stranger,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, listed) = send(&app, authed("GET", "/api/v2/mcp-servers", &stranger)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(listed.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn mcp_server_names_are_unique_per_owner() {
    let app = app().await;
    let token = register_and_login(&app, "mcp-dupe@example.test").await;

    let body = json!({"name": "Weather", "transport": "stdio", "command": "node"});
    let (status, _) = send(
        &app,
        authed_json("POST", "/api/v2/mcp-servers", &token, body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, error) = send(
        &app,
        authed_json("POST", "/api/v2/mcp-servers", &token, body),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
}

#[tokio::test]
async fn testing_an_unreachable_server_reports_the_reason_without_failing_the_request() {
    let app = app().await;
    let token = register_and_login(&app, "mcp-test-fail@example.test").await;

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({
                "name": "Missing binary",
                "transport": "stdio",
                "command": "definitely-not-an-installed-binary-xyzzy",
                "timeout_seconds": 5,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let server_id = created["id"].as_str().unwrap();

    let (status, result) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/mcp-servers/{server_id}/test"),
            &token,
            json!({}),
        ),
    )
    .await;

    // The settings screen needs the reason, so a dead server is a 200 with
    // `ok: false` rather than an HTTP error the UI would render as a crash.
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["ok"], false);
    assert!(!result["error"].as_str().unwrap_or_default().is_empty());
}

// ---------------------------------------------------------------------------
// stdio transport, end to end against a real child process
// ---------------------------------------------------------------------------

/// Config pointing at this test binary re-invoked as a fake MCP stdio server.
fn fake_stdio_config(name: &str, mode: &str) -> McpServerConfig {
    let exe = std::env::current_exe().expect("current test binary path");
    let mut env = std::collections::BTreeMap::new();
    env.insert("MCP_FAKE_SERVER".to_string(), mode.to_string());
    McpServerConfig {
        id: format!("fake-{mode}"),
        name: name.to_string(),
        transport: McpTransportKind::Stdio,
        command: Some(exe.to_string_lossy().into_owned()),
        args: vec![
            "--exact".to_string(),
            "mcp_servers_fake_stdio_entrypoint".to_string(),
            "--nocapture".to_string(),
        ],
        env,
        cwd: None,
        url: None,
        headers: Default::default(),
        timeout_seconds: 20,
        tool_filter: Vec::new(),
    }
}

#[tokio::test]
async fn stdio_transport_lists_and_calls_tools() {
    let config = fake_stdio_config("Fake stdio", "ok");
    let client = McpClient::connect(&config).await.expect("connect");

    assert_eq!(client.server_label(), Some("fake-mcp@0.1.0"));

    let tools = client.list_tools(&config).await.expect("list tools");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, vec!["echo", "fail"]);
    assert_eq!(tools[0].description, "Echo the input back.");
    assert_eq!(
        tools[0].input_schema["properties"]["text"]["type"],
        "string"
    );

    let outcome = client
        .call_tool("echo", &json!({"text": "hello"}))
        .await
        .expect("call tool");
    assert_eq!(outcome.text, "echo: hello");
    assert!(!outcome.is_error);

    // A tool that reports failure comes back as a readable error result, not a
    // transport error that would abort the agent's turn.
    let outcome = client.call_tool("fail", &json!({})).await.expect("call");
    assert!(outcome.is_error);
    assert_eq!(outcome.text, "the tool refused");

    client.close().await;
}

#[tokio::test]
async fn stdio_tool_filter_narrows_what_the_agent_can_see() {
    let mut config = fake_stdio_config("Filtered", "ok");
    config.tool_filter = vec!["echo".to_string()];

    let client = McpClient::connect(&config).await.expect("connect");
    let tools = client.list_tools(&config).await.expect("list tools");
    client.close().await;

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
}

#[tokio::test]
async fn stdio_pagination_is_followed_to_the_last_page() {
    let config = fake_stdio_config("Paged", "paged");
    let client = McpClient::connect(&config).await.expect("connect");

    let tools = client.list_tools(&config).await.expect("list tools");
    client.close().await;

    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, vec!["page_one_tool", "page_two_tool"]);
}

#[tokio::test]
async fn a_server_that_exits_during_the_handshake_reports_its_own_error_text() {
    let config = fake_stdio_config("Crashes", "crash");

    let error = McpClient::connect(&config)
        .await
        .expect_err("handshake should fail");

    let message = error.to_string();
    assert!(message.contains("exited"), "{message}");
    // The child's stderr is what tells the operator *why* it died.
    assert!(message.contains("boom: missing configuration"), "{message}");
}

#[tokio::test]
async fn the_manager_reuses_one_connection_and_drops_it_on_eviction() {
    let manager = McpManager::new();
    let config = fake_stdio_config("Pooled", "ok");

    let first = manager.client(&config).await.expect("first connect");
    let second = manager.client(&config).await.expect("pooled");
    assert!(
        Arc::ptr_eq(&first, &second),
        "the pool should hand back the same connection"
    );

    manager.evict(&config.id).await;

    let third = manager.client(&config).await.expect("reconnect");
    assert!(!Arc::ptr_eq(&first, &third), "eviction forces a reconnect");

    // `first`/`second` are still held here, so eviction must not have closed
    // them — see `evicting_a_pooled_server_does_not_kill_a_call_another_holder_is_making`.
    first.close().await;
    manager.shutdown().await;
}

#[tokio::test]
async fn editing_a_server_config_forces_a_fresh_connection() {
    let manager = McpManager::new();
    let mut config = fake_stdio_config("Edited", "ok");

    let first = manager.client(&config).await.expect("first connect");
    // Same id, different settings: the pooled connection points at the old
    // command and must not be handed out again.
    config.timeout_seconds = 30;
    let second = manager.client(&config).await.expect("reconnect");

    assert!(!Arc::ptr_eq(&first, &second));
    manager.shutdown().await;
}

/// The fake MCP stdio server. Runs only when `MCP_FAKE_SERVER` is set, so a
/// normal `cargo test` run treats it as a no-op test.
#[test]
fn mcp_servers_fake_stdio_entrypoint() {
    use std::io::{BufRead, Write};

    let Ok(mode) = std::env::var("MCP_FAKE_SERVER") else {
        return;
    };

    if mode == "crash" {
        eprintln!("boom: missing configuration");
        std::process::exit(3);
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let Some(id) = message.get("id").cloned().filter(|id| !id.is_null()) else {
            // A notification; nothing to answer.
            continue;
        };

        let result = match method {
            "initialize" => json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "fake-mcp", "version": "0.1.0"},
            }),
            "tools/list" => fake_tools_list(&mode, message.get("params")),
            "tools/call" => fake_tools_call(message.get("params")),
            _ => {
                let error = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("method not found: {method}")},
                });
                writeln!(stdout, "{error}").ok();
                stdout.flush().ok();
                continue;
            }
        };

        let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
        writeln!(stdout, "{response}").ok();
        stdout.flush().ok();
    }
}

/// The `tools/list` result for the fake server, honouring the `paged` mode.
fn fake_tools_list(mode: &str, params: Option<&Value>) -> Value {
    let cursor = params
        .and_then(|params| params.get("cursor"))
        .and_then(Value::as_str);

    if mode == "paged" {
        return match cursor {
            None => json!({
                "tools": [tool_schema("page_one_tool", "First page.")],
                "nextCursor": "page-2",
            }),
            Some(_) => json!({"tools": [tool_schema("page_two_tool", "Second page.")]}),
        };
    }

    json!({"tools": [
        tool_schema("echo", "Echo the input back."),
        tool_schema("fail", "Always reports an error."),
    ]})
}

fn tool_schema(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
        },
    })
}

/// The `tools/call` result for the fake server.
fn fake_tools_call(params: Option<&Value>) -> Value {
    let name = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = params
        .and_then(|params| params.get("arguments"))
        .and_then(|args| args.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    match name {
        "echo" => json!({"content": [{"type": "text", "text": format!("echo: {text}")}]}),
        "fail" => json!({
            "content": [{"type": "text", "text": "the tool refused"}],
            "isError": true,
        }),
        other => json!({
            "content": [{"type": "text", "text": format!("no such tool: {other}")}],
            "isError": true,
        }),
    }
}

// ---------------------------------------------------------------------------
// HTTP transports, end to end against a real local server
// ---------------------------------------------------------------------------

/// Shared state of the fake HTTP MCP server: one sender per open SSE stream.
#[derive(Clone, Default)]
struct FakeHttpState {
    /// Session id → the SSE stream to push responses onto.
    sessions: Arc<StdMutex<HashMap<String, UnboundedSender<String>>>>,
    /// Whether the streamable-HTTP endpoint answers with SSE instead of JSON.
    stream_responses: bool,
}

/// Bind a fake MCP HTTP server on a loopback port and return its base URL.
async fn spawn_fake_http_server(stream_responses: bool) -> SocketAddr {
    let state = FakeHttpState {
        sessions: Arc::new(StdMutex::new(HashMap::new())),
        stream_responses,
    };
    let router = Router::new()
        .route("/mcp", post(streamable_http_handler))
        .route("/sse", get(sse_stream_handler))
        .route("/messages", post(sse_message_handler))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });
    addr
}

/// Streamable HTTP: one endpoint answering with JSON or with an SSE stream.
async fn streamable_http_handler(
    State(state): State<FakeHttpState>,
    Json(message): Json<Value>,
) -> Response {
    let Some(response) = handle_rpc(&message) else {
        // A notification gets an accepted-with-no-body, as the spec requires.
        return StatusCode::ACCEPTED.into_response();
    };

    if state.stream_responses {
        let body = format!("event: message\ndata: {response}\n\n");
        let stream = stream::once(async move { Ok::<_, Infallible>(Bytes::from(body)) });
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header("mcp-session-id", "session-abc")
            .body(Body::from_stream(stream))
            .unwrap();
    }

    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-session-id", "session-abc")
        .body(Body::from(response.to_string()))
        .unwrap()
}

/// Legacy SSE: announce the message endpoint, then hold the stream open.
async fn sse_stream_handler(State(state): State<FakeHttpState>) -> Response {
    let (tx, rx) = unbounded_channel::<String>();
    state
        .sessions
        .lock()
        .unwrap()
        .insert("session-abc".to_string(), tx);

    let body = stream::unfold(
        (true, rx),
        |(first, mut rx): (bool, tokio::sync::mpsc::UnboundedReceiver<String>)| async move {
            if first {
                let announcement =
                    "event: endpoint\ndata: /messages?sessionId=session-abc\n\n".to_string();
                return Some((Ok::<_, Infallible>(Bytes::from(announcement)), (false, rx)));
            }
            let payload = rx.recv().await?;
            let event = format!("event: message\ndata: {payload}\n\n");
            Some((Ok::<_, Infallible>(Bytes::from(event)), (false, rx)))
        },
    );

    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .unwrap()
}

/// Legacy SSE: accept a posted request and push its response onto the stream.
async fn sse_message_handler(
    State(state): State<FakeHttpState>,
    Json(message): Json<Value>,
) -> Response {
    if let Some(response) = handle_rpc(&message) {
        let sender = state.sessions.lock().unwrap().get("session-abc").cloned();
        if let Some(sender) = sender {
            sender.send(response.to_string()).ok();
        }
    }
    StatusCode::ACCEPTED.into_response()
}

/// Answer one JSON-RPC message, or `None` when it is a notification.
fn handle_rpc(message: &Value) -> Option<Value> {
    let id = message.get("id").cloned().filter(|id| !id.is_null())?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "fake-http-mcp", "version": "2.0.0"},
        }),
        "tools/list" => json!({"tools": [tool_schema("echo", "Echo the input back.")]}),
        "tools/call" => fake_tools_call(message.get("params")),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"},
            }))
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn http_config(name: &str, transport: McpTransportKind, url: String) -> McpServerConfig {
    McpServerConfig {
        id: format!("http-{name}"),
        name: name.to_string(),
        transport,
        command: None,
        args: Vec::new(),
        env: Default::default(),
        cwd: None,
        url: Some(url),
        headers: Default::default(),
        timeout_seconds: 20,
        tool_filter: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streamable_http_transport_lists_and_calls_tools_over_json() {
    let addr = spawn_fake_http_server(false).await;
    let config = http_config(
        "Streamable",
        McpTransportKind::StreamableHttp,
        format!("http://{addr}/mcp"),
    );

    let client = McpClient::connect(&config).await.expect("connect");
    assert_eq!(client.server_label(), Some("fake-http-mcp@2.0.0"));

    let tools = client.list_tools(&config).await.expect("list tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");

    let outcome = client
        .call_tool("echo", &json!({"text": "over http"}))
        .await
        .expect("call tool");
    assert_eq!(outcome.text, "echo: over http");

    client.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streamable_http_transport_reads_responses_delivered_as_a_stream() {
    // The same endpoint may answer with `text/event-stream`; both forms are
    // valid and the client must handle either.
    let addr = spawn_fake_http_server(true).await;
    let config = http_config(
        "Streaming",
        McpTransportKind::StreamableHttp,
        format!("http://{addr}/mcp"),
    );

    let client = McpClient::connect(&config).await.expect("connect");
    let outcome = client
        .call_tool("echo", &json!({"text": "streamed"}))
        .await
        .expect("call tool");
    client.close().await;

    assert_eq!(outcome.text, "echo: streamed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_sse_transport_completes_the_endpoint_handshake_and_calls_tools() {
    let addr = spawn_fake_http_server(false).await;
    let config = http_config(
        "Legacy SSE",
        McpTransportKind::Sse,
        format!("http://{addr}/sse"),
    );

    let client = McpClient::connect(&config).await.expect("connect");
    assert_eq!(client.server_label(), Some("fake-http-mcp@2.0.0"));

    let tools = client.list_tools(&config).await.expect("list tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");

    let outcome = client
        .call_tool("echo", &json!({"text": "over sse"}))
        .await
        .expect("call tool");
    assert_eq!(outcome.text, "echo: over sse");

    client.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http_endpoint_that_is_not_an_mcp_server_fails_with_a_readable_reason() {
    let addr = spawn_fake_http_server(false).await;
    // `/sse` is the SSE stream, not the streamable-HTTP endpoint; pointing the
    // wrong transport at it is the most common configuration mistake.
    let config = http_config(
        "Wrong transport",
        McpTransportKind::StreamableHttp,
        format!("http://{addr}/sse"),
    );

    let error = McpClient::connect(&config)
        .await
        .expect_err("should not connect");

    let message = error.to_string();
    assert!(
        message.contains("405") || message.contains("unreachable"),
        "{message}"
    );
}

// ---------------------------------------------------------------------------
// Regressions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn editing_an_unrelated_field_keeps_the_stored_header_value() {
    // Header values are masked on the way out, so the form cannot send the real
    // secret back. An untouched header must therefore survive an edit to any
    // other field rather than being overwritten with the blank box the operator
    // sees.
    let app = app().await;
    let token = register_and_login(&app, "mcp-header-keep@example.test").await;

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({
                "name": "Secured",
                "transport": "streamable-http",
                "url": "https://mcp.example.test/mcp",
                "headers": {"Authorization": "Bearer sk-live-abcdefgh"},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let server_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["headers_masked"]["Authorization"], "****efgh");

    // `null` is the form saying "I did not retype this one".
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &token,
            json!({"timeout_seconds": 120, "headers": {"Authorization": null}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["timeout_seconds"], 120);
    // Still the same secret, not a mask and not an empty string.
    assert_eq!(updated["headers_masked"]["Authorization"], "****efgh");
}

#[tokio::test]
async fn a_retyped_header_replaces_the_stored_value_and_an_omitted_one_is_deleted() {
    let app = app().await;
    let token = register_and_login(&app, "mcp-header-rotate@example.test").await;

    let (_, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({
                "name": "Rotating",
                "transport": "streamable-http",
                "url": "https://mcp.example.test/mcp",
                "headers": {"Authorization": "Bearer old-1234", "X-Trace": "on"},
            }),
        ),
    )
    .await;
    let server_id = created["id"].as_str().unwrap().to_string();

    // Rotate one; X-Trace is absent from the map, which is how a revoked header
    // is deleted.
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &token,
            json!({"headers": {"Authorization": "Bearer new-5678"}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["headers_masked"]["Authorization"], "****5678");
    assert!(
        updated["headers_masked"].get("X-Trace").is_none(),
        "{updated}"
    );

    // An empty map clears every header, so a credential can be revoked.
    let (status, cleared) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &token,
            json!({"headers": {}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["headers_masked"].as_object().unwrap().len(), 0);
}

#[tokio::test]
async fn a_keep_entry_for_a_header_that_was_never_stored_is_dropped() {
    // A half-filled form must not write a blank Authorization that would fail
    // later as a confusing 401.
    let app = app().await;
    let token = register_and_login(&app, "mcp-header-blank@example.test").await;

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({
                "name": "Blank header",
                "transport": "sse",
                "url": "https://mcp.example.test/sse",
                "headers": {"Authorization": null},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["headers_masked"].as_object().unwrap().len(), 0);
}

#[tokio::test]
async fn names_that_slugify_identically_are_rejected() {
    // Tool names are namespaced by the slug, and slugification is lossy. Two
    // servers sharing a slug would produce identical mcp__<slug>__* tool names,
    // leaving the model unable to address one of them.
    let app = app().await;
    let token = register_and_login(&app, "mcp-slug@example.test").await;

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Notion (work)", "transport": "stdio", "command": "node"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, error) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Notion-work", "transport": "stdio", "command": "node"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    // A genuinely different name is still fine.
    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Notion personal", "transport": "stdio", "command": "node"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn renaming_a_server_to_its_own_name_is_allowed() {
    // The slug guard must exclude the row being edited, or saving any unrelated
    // field would report a collision with itself.
    let app = app().await;
    let token = register_and_login(&app, "mcp-slug-self@example.test").await;

    let (_, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/mcp-servers",
            &token,
            json!({"name": "Weather", "transport": "stdio", "command": "node"}),
        ),
    )
    .await;
    let server_id = created["id"].as_str().unwrap();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/mcp-servers/{server_id}"),
            &token,
            json!({"timeout_seconds": 90}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["timeout_seconds"], 90);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_sse_endpoint_that_stalls_times_out_instead_of_hanging() {
    // `connect_timeout` covers only the TCP handshake. A server that accepts the
    // socket and then never writes response headers must still surface a
    // timeout, or the turn would park forever.
    let addr = spawn_stalling_server().await;
    let mut config = http_config(
        "Stalling",
        McpTransportKind::Sse,
        format!("http://{addr}/sse"),
    );
    config.timeout_seconds = 2;

    let started = std::time::Instant::now();
    let error = McpClient::connect(&config)
        .await
        .expect_err("a stalled endpoint must not connect");

    assert!(
        matches!(error, McpError::Timeout(_)),
        "expected a timeout, got {error}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "connect should give up on its own timeout rather than hang"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn evicting_a_pooled_server_does_not_kill_a_call_another_holder_is_making() {
    // `close()` kills the child and drops every pending request. Doing that to a
    // handle another turn is holding would fail an unrelated agent mid-call, so
    // eviction must only unpool.
    let manager = McpManager::new();
    let config = fake_stdio_config("Shared", "ok");

    let held = manager.client(&config).await.expect("connect");
    manager.evict(&config.id).await;

    assert!(held.is_alive(), "eviction closed a connection still in use");
    let outcome = held
        .call_tool("echo", &json!({"text": "still here"}))
        .await
        .expect("the held connection should still work");
    assert_eq!(outcome.text, "echo: still here");

    // The pool really did forget it, so the next caller reconnects.
    let fresh = manager.client(&config).await.expect("reconnect");
    assert!(!Arc::ptr_eq(&held, &fresh));

    held.close().await;
    manager.shutdown().await;
}

/// Bind a server that accepts connections and then never answers.
async fn spawn_stalling_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            // Hold the socket open without writing a response.
            held.push(stream);
        }
    });
    addr
}
