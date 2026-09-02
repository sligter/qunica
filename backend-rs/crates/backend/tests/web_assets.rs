//! The server release archive and the Docker image both stage built frontend
//! assets next to the binary, so the backend has to serve them on the API
//! origin.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

const INDEX_HTML: &str = "<!doctype html><title>Qunica</title>";

async fn app_with_web_dir() -> (tempfile::TempDir, axum::Router) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), INDEX_HTML).unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets").join("app.js"), "export {}").unwrap();

    let (_, state) = qunica_backend::api::router_with_state_for_tests().await;
    let router = qunica_backend::server::build_router(state, Some(dir.path()));
    (dir, router)
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn web_root_serves_the_index_html() {
    let (_dir, app) = app_with_web_dir().await;
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, INDEX_HTML);
}

#[tokio::test]
async fn web_assets_are_served_from_disk() {
    let (_dir, app) = app_with_web_dir().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/assets/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, "export {}");
}

#[tokio::test]
async fn web_client_routes_fall_back_to_the_index_html() {
    let (_dir, app) = app_with_web_dir().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/groups/some-group-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, INDEX_HTML);
}

#[tokio::test]
async fn web_fallback_does_not_shadow_the_api() {
    let (_dir, app) = app_with_web_dir().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("qunica-backend"));
}

#[tokio::test]
async fn web_fallback_does_not_turn_unknown_api_routes_into_html() {
    let (_dir, app) = app_with_web_dir().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn web_dir_is_optional() {
    let (_, state) = qunica_backend::api::router_with_state_for_tests().await;
    let app = qunica_backend::server::build_router(state, None);
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
