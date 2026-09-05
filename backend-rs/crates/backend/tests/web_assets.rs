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
async fn pwa_entrypoints_are_served_as_assets_with_browser_compatible_mime_types() {
    let (dir, app) = app_with_web_dir().await;
    for (name, source, content_type) in [
        ("sw.js", "self.addEventListener('fetch', () => {});", "text/javascript"),
        ("appearance.js", "document.documentElement.dataset.theme = 'light';", "text/javascript"),
        ("manifest.webmanifest", r#"{"name":"Qunica","start_url":"/","display":"standalone"}"#, "application/manifest+json"),
    ] {
        std::fs::write(dir.path().join(name), source).unwrap();
        let response = app.clone().oneshot(
            Request::builder().uri(format!("/{name}")).body(Body::empty()).unwrap(),
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], content_type);
        assert_eq!(body_text(response).await, source);
    }
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

/// The desktop shell holds its frontend inside the executable rather than in a
/// directory, so it hands the backend a resolver instead of a `web_dir`.
async fn app_with_embedded_spa() -> axum::Router {
    let (_, state) = qunica_backend::api::router_with_state_for_tests().await;
    let lookup: qunica_backend::server::SpaAssetLookup =
        std::sync::Arc::new(|path: &str| match path {
            "/" | "/index.html" => Some(qunica_backend::server::SpaAsset {
                bytes: INDEX_HTML.as_bytes().to_vec(),
                mime_type: "text/html".to_string(),
            }),
            "/assets/app.js" => Some(qunica_backend::server::SpaAsset {
                bytes: b"export {}".to_vec(),
                mime_type: "text/javascript".to_string(),
            }),
            _ => None,
        });
    qunica_backend::server::build_router_with_embedded_spa(state, lookup)
}

#[tokio::test]
async fn embedded_spa_serves_the_shell_and_its_assets() {
    for (uri, expected, mime) in [
        ("/", INDEX_HTML, "text/html"),
        ("/assets/app.js", "export {}", "text/javascript"),
    ] {
        let response = app_with_embedded_spa()
            .await
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            mime,
            "{uri}"
        );
        assert_eq!(body_text(response).await, expected, "{uri}");
    }
}

#[tokio::test]
async fn embedded_spa_falls_back_to_the_shell_for_client_routes() {
    let response = app_with_embedded_spa()
        .await
        .oneshot(
            Request::builder()
                .uri("/groups/some-id/chat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, INDEX_HTML);
}

#[tokio::test]
async fn embedded_spa_does_not_shadow_the_api() {
    let app = app_with_embedded_spa().await;
    let response = app
        .clone()
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
