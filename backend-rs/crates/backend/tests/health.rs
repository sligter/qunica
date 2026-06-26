use axum::{body::Body, http::Request};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn health_v1_is_kept_for_tauri_readiness() {
    let app = ag_swarmer_backend::api::router_for_tests();
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
    assert_eq!(payload["service"], "ag-swarmer-backend");
}

#[tokio::test]
async fn health_v2_exists_for_new_api_contract() {
    let app = ag_swarmer_backend::api::router_for_tests();
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
    assert_eq!(payload["service"], "ag-swarmer-backend");
}
