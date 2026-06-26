use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

pub fn json_error(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> impl IntoResponse {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
                details: serde_json::Value::Object(Default::default()),
            },
        }),
    )
}
