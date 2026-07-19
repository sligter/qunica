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
    let found: Option<(String, String)> = sqlx::query_as(
        "SELECT owner_id, status FROM groups \
         WHERE id = ? AND conversation_kind = ? LIMIT 1",
    )
    .bind(id)
    .bind(expected.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    match found {
        None => Err(ApiError::not_found("conversation not found")),
        Some((_, status)) if status != "active" => {
            Err(ApiError::not_found("conversation not found"))
        }
        Some((owner, _)) if owner != owner_id => Err(ApiError::permission_denied(
            "conversation belongs to another user",
        )),
        Some(_) => Ok(()),
    }
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
