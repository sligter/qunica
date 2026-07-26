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

use ag_swarmer_backend::mcp::{
    config::McpServerConfig, McpClient, McpManager, McpTransportKind,
};

// ---------------------------------------------------------------------------
// HTTP test helpers
// ---------------------------------------------------------------------------

async fn app() -> Router {
    ag_swarmer_backend::api::router_for_tests().await
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
        authed("DELETE", &format!("/api/v2/mcp-servers/{server_id}"), &token),
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
        authed("GET", &format!("/api/v2/mcp-servers/{server_id}"), &stranger),
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
    assert_eq!(tools[0].input_schema["properties"]["text"]["type"], "string");

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
    assert!(!first.is_alive(), "eviction should close the connection");

    let third = manager.client(&config).await.expect("reconnect");
    assert!(!Arc::ptr_eq(&first, &third), "eviction forces a reconnect");
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
                return Some((
                    Ok::<_, Infallible>(Bytes::from(announcement)),
                    (false, rx),
                ));
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
        let sender = state
            .sessions
            .lock()
            .unwrap()
            .get("session-abc")
            .cloned();
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
    assert!(message.contains("405") || message.contains("unreachable"), "{message}");
}
