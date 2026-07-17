use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

// Hardcoded valid UUIDs used for UUID-shaped fields (skill ids, provider ids).
const SKILL_A: &str = "11111111-1111-1111-1111-111111111111";
const SKILL_B: &str = "22222222-2222-2222-2222-222222222222";
const PROVIDER_A: &str = "33333333-3333-3333-3333-333333333333";

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

/// Register and log in a user, returning a bearer token.
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

/// Create a cloud-sandbox workspace (no local path required) and return its id.
async fn create_workspace(app: &Router, token: &str) -> String {
    let (status, workspace) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            token,
            json!({"name": "WS", "backend_type": "cloud_sandbox"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    workspace["id"].as_str().unwrap().to_string()
}

async fn create_provider(app: &Router, token: &str, name: &str) -> String {
    let (status, provider) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/llm-providers",
            token,
            json!({
                "name": name,
                "kind": "openai-compatible",
                "base_url": "https://llm.example.test/v1",
                "api_key": format!("secret-{name}-1234"),
                "default_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    provider["id"].as_str().unwrap().to_string()
}

fn acp_capability_runtime(mode: &str, extra_env: Value) -> Value {
    let exe = std::env::current_exe().expect("current test binary path");
    let mut env = serde_json::Map::new();
    env.insert("ACP_API_FAKE_MODE".to_string(), json!(mode));
    if let Value::Object(extra_env) = extra_env {
        env.extend(extra_env);
    }
    json!({
        "profile": "custom",
        "command": exe.to_string_lossy(),
        "args": ["--exact", "agent_acp_capability_fake_child_entrypoint"],
        "env": env,
        "permission_policy": "auto_allow",
        "model": "gpt-5.5",
        "mode": "saved-mode-must-not-apply",
        "thinking_effort": "saved-effort-must-not-apply",
        "config_options": { "saved-option": "must-not-apply" },
    })
}

#[tokio::test]
async fn acp_runtime_capabilities_requires_authentication() {
    let app = app().await;
    let (status, body) = send(
        &app,
        post_json(
            "/api/v2/agents/acp-runtime-capabilities",
            json!({ "command": "should-not-run" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn acp_runtime_capabilities_returns_normalized_response_without_prompt() {
    let app = app().await;
    let token = register_and_login(&app, "acp-capabilities@example.com").await;
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("api-probe.log");
    let request = acp_capability_runtime(
        "success",
        json!({
            "ACP_API_FAKE_LOG": log_path.to_string_lossy(),
            "ACP_API_SECRET": "TOP_SECRET_VALUE",
        }),
    );

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents/acp-runtime-capabilities",
            &token,
            request,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["models"][0]["value"], "gpt-5.5");
    assert_eq!(body["models"][0]["label"], "GPT-5.5");
    assert_eq!(body["models"][0]["description"], Value::Null);
    assert_eq!(body["modes"][0]["value"], "auto");
    assert_eq!(body["thinking_efforts"][0]["value"], "xhigh");
    assert_eq!(body["current_model"], "gpt-5.5");
    assert_eq!(body["current_mode"], "auto");
    assert_eq!(body["current_thinking_effort"], "xhigh");
    assert_eq!(body["source"], "acp");
    assert_eq!(body["warning"], Value::Null);
    assert!(!body.to_string().contains("TOP_SECRET_VALUE"));

    let log = std::fs::read_to_string(log_path).expect("read API fake log");
    assert!(log.find("initialize").unwrap() < log.find("session/new").unwrap());
    assert!(log.contains("session/set_model"));
    assert!(!log.contains("\"method\":\"session/set_mode\""));
    assert!(!log.contains("session/set_config_option"));
    assert!(!log.contains("saved-mode-must-not-apply"));
    assert!(!log.contains("saved-effort-must-not-apply"));
    assert!(!log.contains("saved-option"));
    assert!(!log.contains("session/prompt"));
    assert!(log.contains("ACP_API_FAKE_EXIT"));
}

#[tokio::test]
async fn acp_runtime_capabilities_returns_normalized_warnings_for_failures() {
    let app = app().await;
    let token = register_and_login(&app, "acp-capability-errors@example.com").await;

    let (status, invalid) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents/acp-runtime-capabilities",
            &token,
            json!({ "command": " " }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["source"], "acp");
    assert!(invalid["models"].as_array().unwrap().is_empty());
    assert!(invalid["warning"].as_str().is_some());

    let (status, omitted) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents/acp-runtime-capabilities",
            &token,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(omitted["source"], "acp");
    assert!(omitted["models"].as_array().unwrap().is_empty());
    assert!(omitted["warning"].as_str().is_some());

    let (status, missing) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents/acp-runtime-capabilities",
            &token,
            json!({ "command": "ag-swarmer-definitely-missing-acp-command" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(missing["source"], "acp");
    assert_eq!(
        missing["warning"],
        "Unable to start the configured ACP runtime."
    );
}

#[tokio::test]
async fn acp_runtime_capabilities_redacts_protocol_rejection() {
    let app = app().await;
    let token = register_and_login(&app, "acp-capability-reject@example.com").await;
    let request = acp_capability_runtime("reject", json!({ "ACP_API_SECRET": "TOP_SECRET_VALUE" }));

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents/acp-runtime-capabilities",
            &token,
            request,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["source"], "acp");
    assert_eq!(
        body["warning"],
        "The configured ACP runtime rejected capability discovery."
    );
    assert!(!body.to_string().contains("TOP_SECRET_VALUE"));
}

#[tokio::test]
async fn acp_runtime_capabilities_maps_timeout_and_cleans_up() {
    let app = app().await;
    let token = register_and_login(&app, "acp-capability-timeout@example.com").await;
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("api-timeout.log");
    let request = acp_capability_runtime(
        "timeout",
        json!({
            "ACP_API_FAKE_LOG": log_path.to_string_lossy(),
            "ACP_API_SECRET": "TOP_SECRET_VALUE",
        }),
    );

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents/acp-runtime-capabilities",
            &token,
            request,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["source"], "acp");
    assert_eq!(
        body["warning"],
        "ACP runtime capability discovery timed out after 15 seconds."
    );
    assert!(!body.to_string().contains("TOP_SECRET_VALUE"));
    let log = std::fs::read_to_string(log_path).expect("read API timeout log");
    assert!(log.contains("initialize"));
    assert!(!log.contains("session/prompt"));
    assert!(log.contains("ACP_API_FAKE_EXIT"));
}

#[tokio::test]
async fn agent_create_requires_active_owned_workspace() {
    let app = app().await;
    let token_a = register_and_login(&app, "ownera@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({"name": "Helper", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(agent["workspace_id"], workspace_a);
    assert_eq!(agent["status"], "active");
    assert_eq!(agent["runtime_kind"], "llm_chat");
    assert_eq!(agent["system_prompt"], "You are a helpful AI agent.");

    // A workspace owned by another user cannot be referenced.
    let token_b = register_and_login(&app, "ownerb@example.com").await;
    let workspace_b = create_workspace(&app, &token_b).await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({"name": "Trespasser", "workspace_id": workspace_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[test]
fn agent_acp_capability_fake_child_entrypoint() {
    let Ok(mode) = std::env::var("ACP_API_FAKE_MODE") else {
        return;
    };
    run_agent_acp_capability_fake_child(&mode);
    std::process::exit(0);
}

fn run_agent_acp_capability_fake_child(mode: &str) {
    use std::io::BufRead;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        append_agent_acp_fake_log(&message.to_string());
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        match method {
            "initialize" => match mode {
                "timeout" => {}
                "reject" => {
                    let secret = std::env::var("ACP_API_SECRET").unwrap_or_default();
                    write_agent_acp_line(
                        &stdout,
                        &agent_acp_rpc_error(&id, -32602, &format!("rejected {secret}")),
                    );
                }
                _ => write_agent_acp_line(
                    &stdout,
                    &agent_acp_rpc_result(&id, json!({ "protocolVersion": 1 })),
                ),
            },
            "session/new" => {
                let mut options = agent_acp_capability_options("gpt-5.4", "low");
                if mode == "success" {
                    let secret = std::env::var("ACP_API_SECRET").unwrap_or_default();
                    options[0]["options"][0]["description"] = json!(format!("reflected {secret}"));
                }
                write_agent_acp_line(
                    &stdout,
                    &agent_acp_rpc_result(
                        &id,
                        json!({
                            "sessionId": "sess-api",
                            "configOptions": options,
                        }),
                    ),
                );
            }
            "session/set_model" | "session/set_config_option" => {
                let mut options = agent_acp_capability_options("gpt-5.5", "xhigh");
                if mode == "success" {
                    let secret = std::env::var("ACP_API_SECRET").unwrap_or_default();
                    options[0]["options"][0]["description"] = json!(format!("reflected {secret}"));
                }
                write_agent_acp_line(
                    &stdout,
                    &agent_acp_rpc_result(&id, json!({ "configOptions": options })),
                );
            }
            _ => {
                if matches!(id, Some(ref value) if !value.is_null()) {
                    write_agent_acp_line(
                        &stdout,
                        &agent_acp_rpc_error(&id, -32601, "method not found"),
                    );
                }
            }
        }
    }
    append_agent_acp_fake_log("ACP_API_FAKE_EXIT");
}

fn agent_acp_capability_options(model: &str, effort: &str) -> Vec<Value> {
    vec![
        json!({
            "id": "model",
            "category": "model",
            "type": "select",
            "currentValue": model,
            "options": [{ "value": "gpt-5.5", "name": "GPT-5.5" }],
        }),
        json!({
            "id": "approval_preset",
            "type": "select",
            "currentValue": "auto",
            "options": [{ "value": "auto", "name": "Default" }],
        }),
        json!({
            "id": "reasoning_effort",
            "category": "thought_level",
            "type": "select",
            "currentValue": effort,
            "options": [{ "value": "xhigh", "name": "XHigh" }],
        }),
    ]
}

fn agent_acp_rpc_result(id: &Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.clone().unwrap_or(Value::Null), "result": result })
}

fn agent_acp_rpc_error(id: &Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.clone().unwrap_or(Value::Null),
        "error": { "code": code, "message": message },
    })
}

fn write_agent_acp_line(stdout: &std::io::Stdout, value: &Value) {
    use std::io::Write;

    let mut handle = stdout.lock();
    let _ = writeln!(handle, "{value}");
    let _ = handle.flush();
}

fn append_agent_acp_fake_log(line: &str) {
    use std::io::Write;

    let Ok(path) = std::env::var("ACP_API_FAKE_LOG") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

#[tokio::test]
async fn agent_create_validates_provider_owner_and_status() {
    let app = app().await;
    let token_a = register_and_login(&app, "provider-create-a@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let provider_a = create_provider(&app, &token_a, "Owner A").await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Bound",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_a
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(agent["llm_provider_id"], provider_a);

    let token_b = register_and_login(&app, "provider-create-b@example.com").await;
    let provider_b = create_provider(&app, &token_b, "Owner B").await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Trespasser",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_b
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/llm-providers/{provider_a}"),
            &token_a,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Deleted Provider",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_a
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn agent_list_is_owner_scoped() {
    let app = app().await;
    let token_a = register_and_login(&app, "lista@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({"name": "A's Agent", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let token_b = register_and_login(&app, "listb@example.com").await;
    let (status, list_b) = send(&app, authed("GET", "/api/v2/agents", &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let b_ids: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(!b_ids.contains(&agent_id.as_str()));
}

#[tokio::test]
async fn agent_patch_updates_name_and_json_fields() {
    let app = app().await;
    let token = register_and_login(&app, "patch@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token,
            json!({"name": "Before", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token,
            json!({
                "name": "After",
                "llm_config": {"model": "claude", "temperature": 0.5},
                "tool_config": {"enabled": ["search"]},
                "skill_ids": [SKILL_A, SKILL_B],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "After");
    assert_eq!(
        updated["llm_config"],
        json!({"model": "claude", "temperature": 0.5})
    );
    assert_eq!(updated["tool_config"], json!({"enabled": ["search"]}));
    assert_eq!(updated["skill_ids"], json!([SKILL_A, SKILL_B]));

    // Values round-trip through a fresh GET.
    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["name"], "After");
    assert_eq!(
        fetched["llm_config"],
        json!({"model": "claude", "temperature": 0.5})
    );
    assert_eq!(fetched["tool_config"], json!({"enabled": ["search"]}));
    assert_eq!(fetched["skill_ids"], json!([SKILL_A, SKILL_B]));
}

#[tokio::test]
async fn agent_update_validates_provider_owner_and_status() {
    let app = app().await;
    let token_a = register_and_login(&app, "provider-update-a@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let provider_a = create_provider(&app, &token_a, "Original").await;
    let provider_a_next = create_provider(&app, &token_a, "Replacement").await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token_a,
            json!({
                "name": "Switchable",
                "workspace_id": workspace_a,
                "llm_provider_id": provider_a
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token_a,
            json!({"llm_provider_id": provider_a_next}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["llm_provider_id"], provider_a_next);

    let token_b = register_and_login(&app, "provider-update-b@example.com").await;
    let provider_b = create_provider(&app, &token_b, "Foreign").await;

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token_a,
            json!({"llm_provider_id": provider_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/llm-providers/{provider_a_next}"),
            &token_a,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token_a,
            json!({"llm_provider_id": provider_a_next}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn agent_runtime_kind_acp_clears_provider_and_stores_runtime() {
    let app = app().await;
    let token = register_and_login(&app, "acp@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    // Create an ACP agent with a provider and runtime: provider must be cleared.
    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token,
            json!({
                "name": "ACP Agent",
                "workspace_id": workspace,
                "runtime_kind": "acp",
                "llm_provider_id": PROVIDER_A,
                "acp_runtime": {"command": "claude-acp", "args": ["--flag"]},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();
    assert_eq!(agent["runtime_kind"], "acp");
    assert_eq!(agent["llm_provider_id"], Value::Null);
    assert_eq!(
        agent["acp_runtime"],
        json!({"command": "claude-acp", "args": ["--flag"]})
    );

    // Patching the same ACP agent with another provider must keep it cleared.
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            &token,
            json!({
                "llm_provider_id": PROVIDER_A,
                "acp_runtime": {"command": "claude-acp", "args": ["--v2"]},
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["llm_provider_id"], Value::Null);
    assert_eq!(
        updated["acp_runtime"],
        json!({"command": "claude-acp", "args": ["--v2"]})
    );
}

#[tokio::test]
async fn acp_runtime_presets_include_pi_and_opencode() {
    let app = app().await;
    let token = register_and_login(&app, "acp-presets@example.com").await;

    let (status, body) = send(
        &app,
        authed("GET", "/api/v2/agents/acp-runtime-presets", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let presets = body["presets"].as_array().expect("presets array");
    let ids: Vec<&str> = presets
        .iter()
        .filter_map(|preset| preset["id"].as_str())
        .collect();

    assert!(ids.contains(&"codex"), "ids: {ids:?}");
    assert!(ids.contains(&"claude"), "ids: {ids:?}");
    assert!(ids.contains(&"pi"), "ids: {ids:?}");
    assert!(ids.contains(&"opencode"), "ids: {ids:?}");

    let pi = presets.iter().find(|preset| preset["id"] == "pi").unwrap();
    assert_eq!(pi["profile"], "pi");
    assert_eq!(pi["permission_policy"], "deny");
    assert_eq!(pi["timeout_seconds"], 3600);
    if pi["installed"].as_bool().unwrap_or(false) {
        let command = pi["command"].as_str().expect("pi command");
        assert!(
            command.to_ascii_lowercase().contains("pi-acp"),
            "command: {command}"
        );
        assert_eq!(pi["args"], json!([]));
    } else {
        assert_ne!(pi["command"], Value::Null);
        assert_eq!(pi["args"], json!(["-y", "pi-acp"]));
    }

    let opencode = presets
        .iter()
        .find(|preset| preset["id"] == "opencode")
        .unwrap();
    assert_eq!(opencode["profile"], "opencode");
    assert_eq!(opencode["permission_policy"], "deny");
    assert_eq!(opencode["timeout_seconds"], 3600);
    assert_eq!(opencode["args"], json!(["acp"]));
    let opencode_command = opencode["command"].as_str().expect("opencode command");
    if opencode["installed"].as_bool().unwrap_or(false) {
        assert!(
            opencode_command.to_ascii_lowercase().contains("opencode"),
            "command: {opencode_command}"
        );
    } else {
        assert_eq!(opencode_command, "opencode");
    }
}

#[tokio::test]
async fn agent_delete_soft_deletes_and_hides_from_list() {
    let app = app().await;
    let token = register_and_login(&app, "delete@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    let (status, agent) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/agents",
            &token,
            json!({"name": "Doomed", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    // Get now returns 404.
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    // List omits it.
    let (status, list) = send(&app, authed("GET", "/api/v2/agents", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&agent_id.as_str()));
}
