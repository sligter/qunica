use sqlx::SqlitePool;

use crate::api::error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    Group,
    Direct,
}

impl ConversationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Direct => "direct",
        }
    }
}

pub async fn ensure_active_owned_conversation(
    pool: &SqlitePool,
    id: &str,
    owner_id: &str,
    expected: ConversationKind,
) -> Result<(), ApiError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM groups \
         WHERE id = ? AND owner_id = ? AND status = 'active' AND conversation_kind = ? \
         LIMIT 1",
    )
    .bind(id)
    .bind(owner_id)
    .bind(expected.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    found
        .map(|_| ())
        .ok_or_else(|| ApiError::not_found("conversation not found"))
}

pub async fn ensure_active_owned_workspace_conversation(
    pool: &SqlitePool,
    id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM groups \
         WHERE id = ? AND owner_id = ? AND status = 'active' \
           AND conversation_kind IN ('group', 'direct') \
         LIMIT 1",
    )
    .bind(id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    found
        .map(|_| ())
        .ok_or_else(|| ApiError::not_found("conversation not found"))
}
