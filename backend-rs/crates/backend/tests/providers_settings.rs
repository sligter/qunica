use std::collections::HashMap;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

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

async fn create_provider(app: &Router, token: &str, name: &str, api_key: &str) -> Value {
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
                "api_key": api_key,
                "default_model": "test-model",
                "context_window_tokens": 32000,
                "context_output_reserve_ratio": 0.25,
                "description": "primary provider",
                "reasoning_passback": true
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    provider
}

#[tokio::test]
async fn providers_settings_provider_crud_is_owner_scoped_and_masks_key() {
    let app = app().await;
    let token_a = register_and_login(&app, "provider-owner-a@example.com").await;
    let provider = create_provider(&app, &token_a, "Primary", "supersecret-0000").await;
    let provider_id = provider["id"].as_str().unwrap().to_string();

    assert_eq!(provider["api_key_masked"], "****0000");
    assert!(!provider.to_string().contains("supersecret-0000"));
    assert_eq!(provider["kind"], "openai-compatible");
    assert_eq!(provider["context_window_tokens"], 32000);
    assert_eq!(provider["context_output_reserve_ratio"], 0.25);

    let short = create_provider(&app, &token_a, "Short Key", "abcd").await;
    assert_eq!(short["api_key_masked"], "****");
    assert!(!short.to_string().contains("abcd"));

    let (status, fetched) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token_a,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["id"], provider_id);
    assert!(!fetched.to_string().contains("supersecret-0000"));

    let (status, list) = send(&app, authed("GET", "/api/v2/llm-providers", &token_a)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|provider| provider["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&provider_id.as_str()));
    assert!(!list.to_string().contains("supersecret-0000"));

    let token_b = register_and_login(&app, "provider-owner-b@example.com").await;
    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token_b,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, list_b) = send(&app, authed("GET", "/api/v2/llm-providers", &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let b_ids: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|provider| provider["id"].as_str().unwrap())
        .collect();
    assert!(!b_ids.contains(&provider_id.as_str()));

    let (status, body) = send(
        &app,
        authed(
            "GET",
            "/api/v2/llm-providers/11111111-1111-1111-1111-111111111111",
            &token_a,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn providers_settings_provider_patch_preserves_key_and_clears_nullable_fields() {
    let app = app().await;
    let token = register_and_login(&app, "provider-patch@example.com").await;
    let provider = create_provider(&app, &token, "Patch Me", "original-secret-1234").await;
    let provider_id = provider["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
            json!({
                "name": "Patched",
                "api_key": "",
                "base_url": Value::Null,
                "context_window_tokens": Value::Null,
                "context_output_reserve_ratio": Value::Null,
                "description": Value::Null,
                "reasoning_passback": false
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "Patched");
    assert_eq!(updated["api_key_masked"], "****1234");
    assert_eq!(updated["base_url"], Value::Null);
    assert_eq!(updated["context_window_tokens"], Value::Null);
    assert_eq!(updated["context_output_reserve_ratio"], Value::Null);
    assert_eq!(updated["description"], Value::Null);
    assert_eq!(updated["reasoning_passback"], false);
    assert!(!updated.to_string().contains("original-secret-1234"));

    let (status, still_preserved) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
            json!({"api_key": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(still_preserved["api_key_masked"], "****1234");

    let (status, changed_key) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
            json!({"api_key": "replacement-secret-9999"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changed_key["api_key_masked"], "****9999");
    assert!(!changed_key.to_string().contains("replacement-secret-9999"));
}

#[tokio::test]
async fn providers_settings_provider_models_use_default_without_network() {
    let app = app().await;
    let token = register_and_login(&app, "provider-models@example.com").await;
    let provider = create_provider(&app, &token, "Models", "model-secret-5678").await;
    let provider_id = provider["id"].as_str().unwrap().to_string();

    let (status, models) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/llm-providers/{provider_id}/models"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models, json!([{"id": "test-model", "name": "test-model"}]));
}

#[tokio::test]
async fn providers_settings_provider_delete_hides_from_list_and_get() {
    let app = app().await;
    let token = register_and_login(&app, "provider-delete@example.com").await;
    let provider = create_provider(&app, &token, "Delete Me", "delete-secret-5678").await;
    let provider_id = provider["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    let (status, list) = send(&app, authed("GET", "/api/v2/llm-providers", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|provider| provider["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&provider_id.as_str()));
}

#[tokio::test]
async fn providers_settings_system_settings_defaults_and_patch_hide_key() {
    let app = app().await;
    let token = register_and_login(&app, "settings-defaults@example.com").await;

    let (status, defaults) = send(&app, authed("GET", "/api/v2/settings/system", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(defaults["owner_id"].as_str().is_some());
    assert_eq!(defaults["group_workspace_root"], Value::Null);
    assert_eq!(defaults["web_search_provider"], "tavily");
    assert_eq!(defaults["tavily_api_key_configured"], false);
    assert_eq!(
        defaults["tavily_search_url"],
        "https://api.tavily.com/search"
    );
    assert_eq!(defaults["tavily_max_results"], 5);
    assert_eq!(defaults["tavily_search_depth"], "basic");
    assert_eq!(defaults["tavily_include_answer"], true);
    assert_eq!(defaults["tavily_include_raw_content"], false);

    let root = tempfile::tempdir().unwrap();
    let raw_root = root.path().to_str().unwrap().to_string();
    let expected_root = std::fs::canonicalize(root.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({
                "group_workspace_root": raw_root,
                "web_search_provider": "tavily",
                "tavily_api_key": "tvly-secret-value",
                "tavily_search_url": "https://search.example.test/query",
                "tavily_max_results": 12,
                "tavily_search_depth": "advanced",
                "tavily_include_answer": false,
                "tavily_include_raw_content": true
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["group_workspace_root"], expected_root);
    assert_eq!(updated["tavily_api_key_configured"], true);
    assert!(!updated.to_string().contains("tvly-secret-value"));
    assert_eq!(
        updated["tavily_search_url"],
        "https://search.example.test/query"
    );
    assert_eq!(updated["tavily_max_results"], 12);
    assert_eq!(updated["tavily_search_depth"], "advanced");
    assert_eq!(updated["tavily_include_answer"], false);
    assert_eq!(updated["tavily_include_raw_content"], true);

    let (status, fetched) = send(&app, authed("GET", "/api/v2/settings/system", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["tavily_api_key_configured"], true);
    assert_eq!(fetched["tavily_search_depth"], "advanced");
    assert!(!fetched.to_string().contains("tvly-secret-value"));

    let (status, reset) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({
                "group_workspace_root": "",
                "tavily_api_key": Value::Null,
                "tavily_search_url": Value::Null,
                "tavily_max_results": Value::Null,
                "tavily_search_depth": Value::Null,
                "tavily_include_answer": Value::Null,
                "tavily_include_raw_content": Value::Null
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["group_workspace_root"], Value::Null);
    assert_eq!(reset["tavily_api_key_configured"], false);
    assert_eq!(reset["tavily_search_url"], "https://api.tavily.com/search");
    assert_eq!(reset["tavily_max_results"], 5);
    assert_eq!(reset["tavily_search_depth"], "basic");
    assert_eq!(reset["tavily_include_answer"], true);
    assert_eq!(reset["tavily_include_raw_content"], false);
}

#[tokio::test]
async fn providers_settings_system_settings_concurrent_get_or_create_is_idempotent() {
    let app = app().await;
    let token = register_and_login(&app, "settings-race@example.com").await;

    let req_a = authed("GET", "/api/v2/settings/system", &token);
    let req_b = authed("GET", "/api/v2/settings/system", &token);
    let req_c = authed("GET", "/api/v2/settings/system", &token);
    let req_d = authed("GET", "/api/v2/settings/system", &token);

    let (first, second, third, fourth) = tokio::join!(
        send(&app, req_a),
        send(&app, req_b),
        send(&app, req_c),
        send(&app, req_d),
    );

    let responses = vec![first, second, third, fourth];
    for (status, body) in &responses {
        assert_eq!(*status, StatusCode::OK);
        assert_eq!(body["web_search_provider"], "tavily");
    }

    let first_id = responses[0].1["id"].as_str().unwrap();
    for (_, body) in &responses[1..] {
        assert_eq!(body["id"], first_id);
    }
}

#[tokio::test]
async fn providers_settings_system_settings_validation_rejects_invalid_values() {
    let app = app().await;
    let token = register_and_login(&app, "settings-validation@example.com").await;

    let invalid_cases = [
        json!({"web_search_provider": "other"}),
        json!({"tavily_search_depth": "deep"}),
        json!({"tavily_max_results": 0}),
        json!({"tavily_max_results": 21}),
        json!({"tavily_search_url": "ftp://example.test/search"}),
        json!({"tavily_search_url": "not-a-url"}),
        json!({"group_workspace_root": "/this/path/does/not/exist/ag-swarmer"}),
    ];

    for body in invalid_cases {
        let (status, response) = send(
            &app,
            authed_json("PATCH", "/api/v2/settings/system", &token, body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response["error"]["code"], "invalid_input");
    }
}

#[tokio::test]
async fn tool_catalog_includes_required_builtin_tools_with_stable_ids() {
    let app = app().await;
    let (status, response) =
        send(&app, authed("GET", "/api/v2/agents/tool-catalog", "token")).await;
    assert_eq!(status, StatusCode::OK);

    let tools = response["tools"].as_array().unwrap();
    let by_id: HashMap<&str, &Value> = tools
        .iter()
        .map(|tool| (tool["id"].as_str().unwrap(), tool))
        .collect();

    for id in [
        "read",
        "write",
        "edit",
        "glob",
        "grep",
        "bash",
        "ask_user",
        "web_search",
        "fetch",
        "run_sub_agent",
        "generate_image",
        "generate_video",
        "skill_manager",
        "todo_write",
        "exit_plan_mode",
    ] {
        assert!(by_id.contains_key(id), "missing tool id {id}");
    }

    assert_eq!(by_id["read"]["name"], "Read");
    assert_eq!(by_id["skill_manager"]["name"], "SkillManager");
    assert_eq!(by_id["todo_write"]["name"], "TodoWrite");
    assert_eq!(by_id["exit_plan_mode"]["name"], "ExitPlanMode");
    assert_eq!(by_id["read"]["requires_workspace"], true);
    assert_eq!(by_id["skill_manager"]["runtime_status"], "available");
}

#[tokio::test]
async fn acp_runtime_presets_include_codex_and_claude_with_options() {
    let app = app().await;
    let (status, response) = send(
        &app,
        authed("GET", "/api/v2/agents/acp-runtime-presets", "token"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let presets = response["presets"].as_array().unwrap();
    let by_id: HashMap<&str, &Value> = presets
        .iter()
        .map(|preset| (preset["id"].as_str().unwrap(), preset))
        .collect();

    for id in ["codex", "claude"] {
        let preset = by_id
            .get(id)
            .unwrap_or_else(|| panic!("missing preset {id}"));
        let installed = preset["installed"].as_bool().unwrap();
        let command = preset["command"].as_str().unwrap();
        if installed {
            assert!(
                command.ends_with("npx")
                    || command.ends_with("npx.cmd")
                    || command.ends_with("npx.exe"),
                "unexpected resolved npx command: {command}"
            );
        } else {
            assert_eq!(command, "npx");
        }
        assert_eq!(preset["source"], "fallback");
        assert!(!preset["args"].as_array().unwrap().is_empty());
        assert!(!preset["mode_options"].as_array().unwrap().is_empty());
        assert!(!preset["thinking_effort_options"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
