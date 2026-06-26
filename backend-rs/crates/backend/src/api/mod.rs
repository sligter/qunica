pub mod error;
pub mod health;

use axum::{routing::get, Router};

#[derive(Clone, Debug, Default)]
pub struct AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health::health))
        .route("/api/v2/health", get(health::health))
        .with_state(state)
}

pub fn router_for_tests() -> Router {
    router(AppState)
}
