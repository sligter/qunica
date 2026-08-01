use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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

/// An error that renders as an [`ErrorEnvelope`] with the matching HTTP status.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_input", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "permission_denied", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }

    /// The HTTP status this error would produce.
    ///
    /// The approval endpoint applies a change by calling a handler core and has
    /// to record and re-raise the failure rather than let it become a 500.
    pub fn status_code(&self) -> StatusCode {
        self.status
    }

    /// The human-readable message, for recording in the action ledger.
    pub fn message_text(&self) -> String {
        self.message.clone()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        json_error(self.status, self.code, self.message).into_response()
    }
}
