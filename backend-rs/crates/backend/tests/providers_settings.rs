use std::{collections::HashMap, convert::Infallible, sync::Arc};

use axum::{
    body::{Body, Bytes},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use futures_util::stream;
use serde_json::{json, Value};
use tokio::sync::Mutex;
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
    create_provider_config(
        app,
        token,
        name,
        "openai-compatible",
        "https://llm.example.test/v1",
        api_key,
        "test-model",
    )
    .await
}

async fn create_provider_config(
    app: &Router,
    token: &str,
    name: &str,
    kind: &str,
    base_url: &str,
    api_key: &str,
    default_model: &str,
) -> Value {
    let (status, provider) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/llm-providers",
            token,
            json!({
                "name": name,
                "kind": kind,
                "base_url": base_url,
                "api_key": api_key,
                "default_model": default_model,
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

#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    uri: String,
    headers: HeaderMap,
}

async fn catalog_server(
    status: StatusCode,
    body: impl Into<String>,
) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind model catalog server");
    let address = listener.local_addr().expect("model catalog address");
    let captures = Arc::new(Mutex::new(Vec::new()));
    let body = body.into();
    let app = Router::new().fallback({
        let captures = Arc::clone(&captures);
        move |request: Request<Body>| {
            let captures = Arc::clone(&captures);
            let body = body.clone();
            async move {
                captures.lock().await.push(CapturedRequest {
                    method: request.method().to_string(),
                    uri: request.uri().to_string(),
                    headers: request.headers().clone(),
                });
                (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve model catalog response");
    });
    (format!("http://{address}"), captures)
}

async fn oversized_chunked_catalog_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind chunked model catalog server");
    let address = listener.local_addr().expect("chunked catalog address");
    let chunks = vec![
        Bytes::from(vec![b'x'; 1024 * 1024]),
        Bytes::from(vec![b'x'; 1024 * 1024]),
        Bytes::from_static(b"x"),
    ];
    let app = Router::new().fallback(move || {
        let chunks = chunks.clone();
        async move {
            let body = Body::from_stream(stream::iter(
                chunks.into_iter().map(Ok::<Bytes, Infallible>),
            ));
            Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap()
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve chunked model catalog response");
    });
    format!("http://{address}")
}

async fn pending_catalog_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pending model catalog server");
    let address = listener.local_addr().expect("pending catalog address");
    let app = Router::new().fallback(|| async move {
        let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    });
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve pending model catalog response");
    });
    format!("http://{address}")
}

async fn redirect_server(location: &str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind redirect model catalog server");
    let address = listener.local_addr().expect("redirect catalog address");
    let location = HeaderValue::from_str(location).expect("redirect location");
    let app = Router::new().fallback(move || {
        let location = location.clone();
        async move {
            let mut response = StatusCode::FOUND.into_response();
            response.headers_mut().insert(header::LOCATION, location);
            response
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve redirect model catalog response");
    });
    format!("http://{address}")
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
async fn providers_settings_provider_multi_model_context_is_persisted() {
    let app = app().await;
    let token = register_and_login(&app, "provider-models@example.com").await;
    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/llm-providers",
            &token,
            json!({
                "name": "Multi",
                "kind": "openai-compatible",
                "base_url": "https://llm.example.test/v1",
                "api_key": "multi-secret",
                "default_model": "model-b",
                "models": [
                    {
                        "id": "model-a",
                        "context_window_tokens": 32000,
                        "context_output_reserve_ratio": 0.2,
                        "reasoning_passback": false
                    },
                    {
                        "id": "model-b",
                        "context_window_tokens": 128000,
                        "context_output_reserve_ratio": 0.3,
                        "reasoning_passback": true
                    }
                ]
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["models"].as_array().unwrap().len(), 2);
    assert_eq!(created["models"][1]["id"], "model-b");
    assert_eq!(created["models"][1]["context_window_tokens"], 128000);
    assert_eq!(created["models"][0]["reasoning_passback"], false);
    assert_eq!(created["models"][1]["reasoning_passback"], true);
    assert_eq!(created["reasoning_passback"], true);
    assert_eq!(created["context_window_tokens"], 128000);
    assert_eq!(created["context_output_reserve_ratio"], 0.3);

    let provider_id = created["id"].as_str().unwrap();
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
            json!({
                "default_model": "model-a",
                "models": created["models"]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["default_model"], "model-a");
    assert_eq!(updated["context_window_tokens"], 32000);
    assert_eq!(updated["reasoning_passback"], false);

    let (status, duplicate) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/llm-providers/{provider_id}"),
            &token,
            json!({
                "models": [{"id": "same"}, {"id": "same"}]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(duplicate["error"]["code"], "invalid_input");
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
async fn providers_settings_provider_models_require_authentication_and_ownership() {
    let (base_url, captures) = catalog_server(
        StatusCode::OK,
        json!({ "data": [{ "id": "must-not-be-fetched" }] }).to_string(),
    )
    .await;
    let app = app().await;
    let owner_token = register_and_login(&app, "provider-model-owner@example.com").await;
    let other_token = register_and_login(&app, "provider-model-other@example.com").await;
    let provider = create_provider_config(
        &app,
        &owner_token,
        "Owned models",
        "openai-compatible",
        &base_url,
        "owner-model-secret",
        "saved-default",
    )
    .await;
    let provider_id = provider["id"].as_str().unwrap();
    let uri = format!("/api/v2/llm-providers/{provider_id}/models");

    let (status, response) = send(
        &app,
        Request::builder()
            .method("GET")
            .uri(&uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(response["error"]["code"], "unauthorized");

    let (status, response) = send(&app, authed("GET", &uri, &other_token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["error"]["code"], "permission_denied");
    assert!(captures.lock().await.is_empty());
}

#[tokio::test]
async fn providers_settings_model_test_uses_the_selected_provider_credentials() {
    let (base_url, captures) = catalog_server(
        StatusCode::OK,
        "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\ndata: [DONE]\n\n",
    )
    .await;
    let app = app().await;
    let owner_token = register_and_login(&app, "provider-test-owner@example.com").await;
    let other_token = register_and_login(&app, "provider-test-other@example.com").await;
    let provider = create_provider_config(
        &app,
        &owner_token,
        "Testable provider",
        "openai-compatible",
        &base_url,
        "saved-test-secret",
        "response-model",
    )
    .await;
    let provider_id = provider["id"].as_str().unwrap();
    let body = json!({
        "provider_id": provider_id,
        "kind": "openai-compatible",
        "base_url": base_url,
        "model": "response-model"
    });

    let (status, response) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/llm-providers/test-model",
            &other_token,
            body.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["error"]["code"], "permission_denied");
    assert!(captures.lock().await.is_empty());

    let (status, response) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/llm-providers/test-model",
            &owner_token,
            body,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["ok"], true);
    assert!(response["latency_ms"].as_u64().is_some());

    let captures = captures.lock().await;
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].method, "POST");
    assert_eq!(captures[0].uri, "/chat/completions");
    assert_eq!(
        captures[0]
            .headers
            .get(header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "Bearer saved-test-secret"
    );
}

#[tokio::test]
async fn providers_settings_model_test_reports_provider_failure_without_leaking_secrets() {
    let secret = "model-test-secret-must-not-leak";
    let (base_url, _) = catalog_server(
        StatusCode::UNAUTHORIZED,
        json!({ "error": secret }).to_string(),
    )
    .await;
    let app = app().await;
    let token = register_and_login(&app, "provider-test-failure@example.com").await;

    let (status, response) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/llm-providers/test-model",
            &token,
            json!({
                "kind": "openai-compatible",
                "base_url": base_url,
                "api_key": secret,
                "model": "response-model"
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["ok"], false);
    assert!(response["error"].as_str().unwrap().contains("401"));
    assert!(!response.to_string().contains(secret));
}

#[tokio::test]
async fn providers_settings_provider_models_openai_discovers_normalizes_and_authenticates() {
    let (base_url, captures) = catalog_server(
        StatusCode::OK,
        json!({
            "data": [
                {"id": "Zulu", "name": "Zulu model"},
                {"id": "alpha"},
                {"id": "Beta", "name": "Beta model"},
                {"id": "alpha", "name": "duplicate is ignored"}
            ]
        })
        .to_string(),
    )
    .await;
    let app = app().await;
    let token = register_and_login(&app, "provider-models-openai@example.com").await;
    let provider = create_provider_config(
        &app,
        &token,
        "OpenAI models",
        "openai-compatible",
        &format!("{base_url}/openai/v1/?api-version=2026-07-16"),
        "openai-secret",
        "custom-default",
    )
    .await;
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
    assert_eq!(
        models,
        json!([
            {"id": "alpha", "name": "alpha"},
            {"id": "Beta", "name": "Beta model"},
            {"id": "custom-default", "name": "custom-default"},
            {"id": "Zulu", "name": "Zulu model"}
        ])
    );

    let captures = captures.lock().await;
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].method, "GET");
    assert_eq!(captures[0].uri, "/openai/v1/models?api-version=2026-07-16");
    assert_eq!(
        captures[0]
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer openai-secret")
    );
    assert!(captures[0].headers.get("x-api-key").is_none());
}

#[tokio::test]
async fn providers_settings_discovers_models_before_provider_is_saved() {
    let (base_url, captures) = catalog_server(
        StatusCode::OK,
        json!({"data": [{"id": "model-b"}, {"id": "model-a"}]}).to_string(),
    )
    .await;
    let app = app().await;
    let token = register_and_login(&app, "provider-discover-unsaved@example.com").await;

    let (status, models) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/llm-providers/discover-models",
            &token,
            json!({
                "kind": "openai-compatible",
                "base_url": base_url,
                "api_key": "unsaved-secret"
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(models[0]["id"], "model-a");
    assert_eq!(models[1]["id"], "model-b");
    let captures = captures.lock().await;
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].headers["authorization"],
        "Bearer unsaved-secret"
    );
}

#[tokio::test]
async fn providers_settings_provider_models_anthropic_kinds_use_v1_headers() {
    let (base_url, captures) = catalog_server(
        StatusCode::OK,
        json!({"data": [{"id": "claude-live", "display_name": "Claude Live"}]}).to_string(),
    )
    .await;
    let app = app().await;
    let token = register_and_login(&app, "provider-models-anthropic@example.com").await;

    for (kind, path, key) in [
        ("anthropic", "/anthropic/v1/models", "anthropic-secret"),
        (
            "anthropic-compatible",
            "/proxy/anthropic/v1/models",
            "compatible-secret",
        ),
    ] {
        let provider = create_provider_config(
            &app,
            &token,
            &format!("{kind} models"),
            kind,
            &format!("{base_url}{}", path.trim_end_matches("/v1/models")),
            key,
            "claude-live",
        )
        .await;
        let provider_id = provider["id"].as_str().unwrap();
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
        assert_eq!(
            models,
            json!([{"id": "claude-live", "name": "Claude Live"}])
        );
    }

    let captures = captures.lock().await;
    assert_eq!(captures.len(), 2);
    for (capture, (path, key)) in captures.iter().zip([
        ("/anthropic/v1/models", "anthropic-secret"),
        ("/proxy/anthropic/v1/models", "compatible-secret"),
    ]) {
        assert_eq!(capture.method, "GET");
        assert_eq!(capture.uri, path);
        assert_eq!(
            capture
                .headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok()),
            Some(key)
        );
        assert_eq!(
            capture
                .headers
                .get("anthropic-version")
                .and_then(|value| value.to_str().ok()),
            Some("2023-06-01")
        );
        assert!(capture.headers.get(header::AUTHORIZATION).is_none());
    }
}

#[tokio::test]
async fn providers_settings_provider_models_gemini_normalizes_filters_and_authenticates() {
    let (base_url, captures) = catalog_server(
        StatusCode::OK,
        json!({
            "models": [
                {
                    "name": "models/gemini-z",
                    "displayName": "Gemini Z",
                    "supportedGenerationMethods": ["generateContent"]
                },
                {
                    "name": "models/gemini-chat",
                    "displayName": "Gemini Chat"
                },
                {
                    "name": "models/text-embedding",
                    "displayName": "Embedding",
                    "supportedGenerationMethods": ["embedContent"]
                },
                {
                    "name": "models/gemini-z",
                    "displayName": "Gemini Z"
                }
            ]
        })
        .to_string(),
    )
    .await;
    let app = app().await;
    let token = register_and_login(&app, "provider-models-gemini@example.com").await;

    for (index, provider_base) in [
        format!("{base_url}/google"),
        format!("{base_url}/google/v1beta/?alt=json&key=stale-key"),
    ]
    .into_iter()
    .enumerate()
    {
        let provider = create_provider_config(
            &app,
            &token,
            &format!("Gemini models {index}"),
            "gemini",
            &provider_base,
            "gemini-secret",
            "custom-gemini",
        )
        .await;
        let provider_id = provider["id"].as_str().unwrap();
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
        assert_eq!(
            models,
            json!([
                {"id": "custom-gemini", "name": "custom-gemini"},
                {"id": "gemini-chat", "name": "Gemini Chat"},
                {"id": "gemini-z", "name": "Gemini Z"}
            ])
        );
        assert!(!models.to_string().contains("gemini-secret"));
    }

    let captures = captures.lock().await;
    assert_eq!(captures.len(), 2);
    for (capture, expected_uri) in captures.iter().zip([
        "/google/v1beta/models?key=gemini-secret",
        "/google/v1beta/models?alt=json&key=gemini-secret",
    ]) {
        assert_eq!(capture.method, "GET");
        assert_eq!(capture.uri, expected_uri);
        assert!(capture.headers.get(header::AUTHORIZATION).is_none());
        assert!(capture.headers.get("x-api-key").is_none());
    }
}

#[tokio::test]
async fn providers_settings_provider_models_reject_malformed_openai_and_anthropic_entries() {
    let app = app().await;
    let token = register_and_login(&app, "provider-model-malformed-entries@example.com").await;

    for (kind, payload) in [
        (
            "openai-compatible",
            json!({ "data": [{ "id": "valid-model" }, { "object": "model" }] }),
        ),
        (
            "anthropic",
            json!({ "data": [{ "id": "valid-model" }, { "id": 42 }] }),
        ),
    ] {
        let (base_url, _) = catalog_server(StatusCode::OK, payload.to_string()).await;
        let provider = create_provider_config(
            &app,
            &token,
            &format!("Malformed {kind}"),
            kind,
            &base_url,
            &format!("malformed-{kind}-secret"),
            "saved-default-must-not-mask-malformed-data",
        )
        .await;
        let provider_id = provider["id"].as_str().unwrap();
        let (status, response) = send(
            &app,
            authed(
                "GET",
                &format!("/api/v2/llm-providers/{provider_id}/models"),
                &token,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(response["error"]["code"], "provider_model_discovery_failed");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("malformed response"));
        assert!(!response
            .to_string()
            .contains("saved-default-must-not-mask-malformed-data"));
    }
}

#[tokio::test]
async fn providers_settings_provider_models_reject_credential_reflection() {
    let app = app().await;
    let token = register_and_login(&app, "provider-model-reflection@example.com").await;

    for (name, api_key, default_model, payload) in [
        (
            "Reflected id",
            "reflected-id-secret",
            "safe-default",
            json!({ "data": [{ "id": "model-reflected-id-secret" }] }),
        ),
        (
            "Reflected name",
            "reflected-name-secret",
            "safe-default",
            json!({ "data": [{ "id": "safe-model", "name": "reflected-name-secret" }] }),
        ),
        (
            "Reflected default",
            "reflected-default-secret",
            "reflected-default-secret",
            json!({ "data": [] }),
        ),
    ] {
        let (base_url, _) = catalog_server(StatusCode::OK, payload.to_string()).await;
        let provider = create_provider_config(
            &app,
            &token,
            name,
            "openai-compatible",
            &base_url,
            api_key,
            default_model,
        )
        .await;
        let provider_id = provider["id"].as_str().unwrap();
        let (status, response) = send(
            &app,
            authed(
                "GET",
                &format!("/api/v2/llm-providers/{provider_id}/models"),
                &token,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(response["error"]["code"], "provider_model_discovery_failed");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("malformed response"));
        assert!(!response.to_string().contains(api_key));
    }
}

#[tokio::test]
async fn providers_settings_provider_model_failures_are_bounded_and_redacted() {
    let (rejected_url, _) = catalog_server(
        StatusCode::UNAUTHORIZED,
        "UPSTREAM_SECRET_RESPONSE model-secret-401",
    )
    .await;
    let (malformed_url, _) = catalog_server(
        StatusCode::OK,
        json!({"unexpected": "UPSTREAM_SECRET_RESPONSE"}).to_string(),
    )
    .await;
    let oversized_url = oversized_chunked_catalog_server().await;
    let app = app().await;
    let token = register_and_login(&app, "provider-model-errors@example.com").await;

    for (name, base_url, api_key, expected_text) in [
        ("Rejected models", rejected_url, "model-secret-401", "(401)"),
        (
            "Malformed models",
            malformed_url,
            "model-secret-malformed",
            "malformed response",
        ),
        (
            "Oversized models",
            oversized_url,
            "model-secret-oversized",
            "2 MiB",
        ),
    ] {
        let provider = create_provider_config(
            &app,
            &token,
            name,
            "openai-compatible",
            &base_url,
            api_key,
            "saved-default",
        )
        .await;
        let provider_id = provider["id"].as_str().unwrap();
        let (status, response) = send(
            &app,
            authed(
                "GET",
                &format!("/api/v2/llm-providers/{provider_id}/models"),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(response["error"]["code"], "provider_model_discovery_failed");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("openai-compatible"));
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains(expected_text));
        let serialized = response.to_string();
        assert!(!serialized.contains(api_key));
        assert!(!serialized.contains("UPSTREAM_SECRET_RESPONSE"));
        assert!(!serialized.contains(&base_url));
    }
}

#[tokio::test]
async fn providers_settings_provider_model_redirects_do_not_forward_credentials() {
    let (destination_url, destination_captures) = catalog_server(
        StatusCode::OK,
        json!({"data": [], "models": []}).to_string(),
    )
    .await;
    let redirect_url = redirect_server(&format!("{destination_url}/credential-sink")).await;
    let app = app().await;
    let token = register_and_login(&app, "provider-model-redirects@example.com").await;

    for (name, kind, api_key, base_url) in [
        (
            "Anthropic redirect",
            "anthropic",
            "anthropic-redirect-secret",
            format!("{redirect_url}/anthropic"),
        ),
        (
            "Gemini redirect",
            "gemini",
            "gemini-redirect-secret",
            format!("{redirect_url}/gemini/v1beta"),
        ),
    ] {
        let provider = create_provider_config(
            &app,
            &token,
            name,
            kind,
            &base_url,
            api_key,
            "saved-default",
        )
        .await;
        let provider_id = provider["id"].as_str().unwrap();
        let (status, response) = send(
            &app,
            authed(
                "GET",
                &format!("/api/v2/llm-providers/{provider_id}/models"),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(response["error"]["code"], "provider_model_discovery_failed");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("(302)"));
        let serialized = response.to_string();
        assert!(!serialized.contains(api_key));
        assert!(!serialized.contains(&base_url));
    }

    assert!(destination_captures.lock().await.is_empty());
}

#[tokio::test]
async fn providers_settings_provider_model_timeout_is_safe_gateway_timeout() {
    let base_url = pending_catalog_server().await;
    let app = app().await;
    let token = register_and_login(&app, "provider-model-timeout@example.com").await;
    let provider = create_provider_config(
        &app,
        &token,
        "Pending models",
        "openai-compatible",
        &base_url,
        "timeout-model-secret",
        "saved-default",
    )
    .await;
    let provider_id = provider["id"].as_str().unwrap();

    let (status, response) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/llm-providers/{provider_id}/models"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(response["error"]["code"], "provider_model_discovery_failed");
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("timed out"));
    let serialized = response.to_string();
    assert!(!serialized.contains("timeout-model-secret"));
    assert!(!serialized.contains(&base_url));
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
    assert_eq!(defaults["appearance"], "system");
    assert_eq!(defaults["assistant_enabled"], true);
    assert_eq!(defaults["assistant_auto_approve"], false);
    assert_eq!(defaults["group_workspace_root"], Value::Null);
    assert_eq!(defaults["shell_preference"], "auto");
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
    assert_eq!(defaults["media_base_url"], "https://api.openai.com");
    assert_eq!(defaults["media_api_key_configured"], false);
    assert_eq!(defaults["image_generation_model"], Value::Null);
    assert_eq!(
        defaults["image_generation_endpoint"],
        "/v1/images/generations"
    );
    assert_eq!(defaults["video_generation_model"], Value::Null);
    assert_eq!(defaults["video_generation_endpoint"], "/v1/videos");
    assert_eq!(defaults["video_status_endpoint"], "/v1/videos/{id}");
    assert_eq!(
        defaults["video_content_endpoint"],
        "/v1/videos/{id}/content"
    );

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
                "appearance": "dark",
                "assistant_enabled": false,
                "assistant_auto_approve": true,
                "group_workspace_root": raw_root,
                "shell_preference": "Git Bash",
                "web_search_provider": "tavily",
                "tavily_api_key": "tvly-secret-value",
                "tavily_search_url": "https://search.example.test/query",
                "tavily_max_results": 12,
                "tavily_search_depth": "advanced",
                "tavily_include_answer": false,
                "tavily_include_raw_content": true,
                "media_base_url": "https://media.example.test/v1",
                "media_api_key": "media-secret-value",
                "image_generation_model": "image-model",
                "image_generation_endpoint": "/v1/images/generations",
                "video_generation_model": "video-model",
                "video_generation_endpoint": "/v1/videos",
                "video_status_endpoint": "/v1/videos/{id}",
                "video_content_endpoint": "/v1/videos/{id}/content"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["appearance"], "dark");
    assert_eq!(updated["assistant_enabled"], false);
    assert_eq!(updated["assistant_auto_approve"], true);
    assert_eq!(updated["group_workspace_root"], expected_root);
    assert_eq!(updated["tavily_api_key_configured"], true);
    assert!(!updated.to_string().contains("tvly-secret-value"));
    // A name a person might type is stored in its canonical form, so the
    // resolver never has to guess what an account meant.
    assert_eq!(updated["shell_preference"], "bash");
    assert_eq!(
        updated["tavily_search_url"],
        "https://search.example.test/query"
    );
    assert_eq!(updated["tavily_max_results"], 12);
    assert_eq!(updated["tavily_search_depth"], "advanced");
    assert_eq!(updated["tavily_include_answer"], false);
    assert_eq!(updated["tavily_include_raw_content"], true);
    assert_eq!(updated["media_base_url"], "https://media.example.test/v1");
    assert_eq!(updated["media_api_key_configured"], true);
    assert_eq!(updated["image_generation_model"], "image-model");
    assert_eq!(updated["video_generation_model"], "video-model");
    assert!(!updated.to_string().contains("media-secret-value"));

    let (status, fetched) = send(&app, authed("GET", "/api/v2/settings/system", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["appearance"], "dark");
    assert_eq!(fetched["assistant_enabled"], false);
    assert_eq!(fetched["assistant_auto_approve"], true);
    assert_eq!(fetched["tavily_api_key_configured"], true);
    assert_eq!(fetched["tavily_search_depth"], "advanced");
    assert_eq!(fetched["shell_preference"], "bash");
    assert!(!fetched.to_string().contains("tvly-secret-value"));
    assert_eq!(fetched["media_api_key_configured"], true);
    assert!(!fetched.to_string().contains("media-secret-value"));

    let (status, reset) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({
                "appearance": Value::Null,
                "assistant_enabled": Value::Null,
                "assistant_auto_approve": Value::Null,
                "group_workspace_root": "",
                "tavily_api_key": Value::Null,
                "tavily_search_url": Value::Null,
                "tavily_max_results": Value::Null,
                "tavily_search_depth": Value::Null,
                "tavily_include_answer": Value::Null,
                "tavily_include_raw_content": Value::Null,
                "shell_preference": Value::Null,
                "media_base_url": Value::Null,
                "media_api_key": Value::Null,
                "image_generation_model": Value::Null,
                "image_generation_endpoint": Value::Null,
                "video_generation_model": Value::Null,
                "video_generation_endpoint": Value::Null,
                "video_status_endpoint": Value::Null,
                "video_content_endpoint": Value::Null
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["appearance"], "system");
    assert_eq!(reset["assistant_enabled"], true);
    assert_eq!(reset["assistant_auto_approve"], false);
    assert_eq!(reset["group_workspace_root"], Value::Null);
    assert_eq!(reset["tavily_api_key_configured"], false);
    assert_eq!(reset["tavily_search_url"], "https://api.tavily.com/search");
    assert_eq!(reset["tavily_max_results"], 5);
    assert_eq!(reset["tavily_search_depth"], "basic");
    assert_eq!(reset["shell_preference"], "auto");
    assert_eq!(reset["tavily_include_answer"], true);
    assert_eq!(reset["tavily_include_raw_content"], false);
    assert_eq!(reset["media_base_url"], "https://api.openai.com");
    assert_eq!(reset["media_api_key_configured"], false);
    assert_eq!(reset["image_generation_model"], Value::Null);
    assert_eq!(reset["image_generation_endpoint"], "/v1/images/generations");
    assert_eq!(reset["video_generation_model"], Value::Null);
    assert_eq!(reset["video_generation_endpoint"], "/v1/videos");
    assert_eq!(reset["video_status_endpoint"], "/v1/videos/{id}");
    assert_eq!(reset["video_content_endpoint"], "/v1/videos/{id}/content");
}

#[tokio::test]
async fn providers_settings_system_settings_language_is_owner_scoped_and_validated() {
    let app = app().await;
    let token = register_and_login(&app, "settings-language@example.com").await;

    let (status, created) = send(&app, authed("GET", "/api/v2/settings/system", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["language"], "en-US");

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"language": "zh-CN"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["language"], "zh-CN");

    let (status, reloaded) = send(&app, authed("GET", "/api/v2/settings/system", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reloaded["language"], "zh-CN");

    let other_token = register_and_login(&app, "settings-language-other@example.com").await;
    let (status, other_account) =
        send(&app, authed("GET", "/api/v2/settings/system", &other_token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(other_account["language"], "en-US");

    let invalid = app
        .clone()
        .oneshot(authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"language": "fr-FR"}),
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let invalid_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(invalid.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(invalid_json["error"]["code"], "invalid_input");

    let (status, reset) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"language": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["language"], "en-US");
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
        json!({"appearance": "sepia"}),
        json!({"web_search_provider": "other"}),
        json!({"tavily_search_depth": "deep"}),
        json!({"shell_preference": "fish"}),
        json!({"tavily_max_results": 0}),
        json!({"tavily_max_results": 21}),
        json!({"tavily_search_url": "ftp://example.test/search"}),
        json!({"tavily_search_url": "not-a-url"}),
        json!({"media_base_url": "ftp://example.test"}),
        json!({"image_generation_endpoint": "not-a-path"}),
        json!({"video_status_endpoint": "/v1/videos/status"}),
        json!({"video_content_endpoint": "ftp://example.test/{id}"}),
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
    assert_eq!(by_id["generate_image"]["requires_workspace"], true);
    assert_eq!(by_id["generate_video"]["requires_workspace"], true);
    assert_eq!(by_id["skill_manager"]["runtime_status"], "available");
    assert_eq!(by_id["run_sub_agent"]["runtime_status"], "planned");
    assert_eq!(by_id["generate_image"]["runtime_status"], "available");
    assert_eq!(by_id["generate_video"]["runtime_status"], "available");

    // App-control tools belong to the built-in Assistant alone. Offering them
    // in the picker would let a user hand a workspace-bound agent with Bash the
    // ability to rewrite the app's configuration too.
    for id in [
        "app_list",
        "app_get",
        "app_state",
        "app_docs",
        "app_propose",
        "app_prefill",
    ] {
        assert!(
            !by_id.contains_key(id),
            "app-control tool {id} must not appear in the agent tool picker"
        );
    }
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
        let is_npx = command.ends_with("npx")
            || command.ends_with("npx.cmd")
            || command.ends_with("npx.exe");
        if installed {
            let is_codex_adapter = command.ends_with("codex-acp")
                || command.ends_with("codex-acp.cmd")
                || command.ends_with("codex-acp.exe");
            assert!(
                is_npx || (id == "codex" && is_codex_adapter),
                "unexpected resolved command for {id}: {command}"
            );
        } else {
            assert!(is_npx, "unexpected npx fallback for {id}: {command}");
        }
        assert_eq!(preset["source"], "fallback");
        let args = preset["args"].as_array().unwrap();
        if command.ends_with("npx") || command.ends_with("npx.cmd") || command.ends_with("npx.exe")
        {
            assert!(
                !args.is_empty(),
                "npx preset {id} requires package arguments"
            );
        }
        assert!(!preset["mode_options"].as_array().unwrap().is_empty());
        assert!(!preset["thinking_effort_options"]
            .as_array()
            .unwrap()
            .is_empty());
        if id == "codex" {
            assert!(preset["thinking_effort_options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["value"] == "max"));
        }
    }
}
