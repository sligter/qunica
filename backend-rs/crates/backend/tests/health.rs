use axum::{
    body::Body,
    http::{header, Method, Request},
};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn health_v1_is_kept_for_tauri_readiness() {
    let app = qunica_backend::api::router_for_tests().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["service"], "qunica-backend");
}

#[tokio::test]
async fn health_v2_exists_for_new_api_contract() {
    let app = qunica_backend::api::router_for_tests().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["service"], "qunica-backend");
}

#[tokio::test]
async fn cors_preflight_allows_desktop_api_headers() {
    let app = qunica_backend::api::router_for_tests().await;
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/v2/health")
                .header(header::ORIGIN, "http://tauri.localhost")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "authorization,content-type,last-event-id",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"http://tauri.localhost".parse().unwrap())
    );
}
