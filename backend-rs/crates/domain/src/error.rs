use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("permission denied")]
    PermissionDenied,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("upstream provider error: {0}")]
    Provider(String),
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Conflict(_) => "conflict",
            Self::InvalidInput(_) => "invalid_input",
            Self::Provider(_) => "provider_error",
        }
    }
}
