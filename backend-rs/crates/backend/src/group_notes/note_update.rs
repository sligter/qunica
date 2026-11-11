//! SQLite authorization barrier for an accepted Notes update.
//! File preparation/publication lives in the shared Notes file lifecycle.
//! See specs/notes-failure-contract.md for failure and external-writer boundaries.
use crate::{
    api::error::ApiError,
    group_notes::{
        access::{
            check_group_note_membership, check_note_write_guard,
            validate_group_note_workspace_root, GroupNoteWorkspaceRow, GroupNoteWriteGuard,
        },
        files::{changed, PreparedUpdate},
    },
};
use sqlx::SqlitePool;

#[derive(sqlx::FromRow)]
struct PublicationRow {
    content: String,
    group_owner_id: String,
    workspace_id: Option<String>,
    #[sqlx(flatten)]
    workspace: GroupNoteWorkspaceRow,
}

pub(super) async fn publish(
    pool: &SqlitePool,
    actor_id: &str,
    group_id: &str,
    expected_owner_id: &str,
    expected_workspace_id: Option<&str>,
    prepared: PreparedUpdate<'_>,
    guard: Option<GroupNoteWriteGuard<'_>>,
) -> Result<(), ApiError> {
    // Note: a second short writer barrier serializes publication with API
    // update/delete, without another mutation/event. Every awaited read uses
    // this connection, including single-connection in-memory pools.
    let mut tx = crate::db::begin_write(pool)
        .await
        .map_err(|_| ApiError::internal("failed to start group note publication transaction"))?;
    let row = sqlx::query_as::<_, PublicationRow>(
        "SELECT n.content, g.owner_id AS group_owner_id, g.workspace_id, \
         w.owner_id, w.backend_type, w.local_path, w.status \
         FROM group_notes n JOIN groups g ON g.id = n.group_id \
         JOIN workspaces w ON w.id = g.workspace_id \
         WHERE n.id = ? AND n.group_id = ? AND n.status = 'active' AND g.status = 'active'",
    )
    .bind(prepared.note_id())
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to check group note publication"))?
    .ok_or_else(changed)?;
    if row.group_owner_id != expected_owner_id
        || row.workspace_id.as_deref() != expected_workspace_id
        || row.content != prepared.content()
    {
        return Err(changed());
    }
    if row.group_owner_id != actor_id {
        check_group_note_membership(&mut *tx, group_id, actor_id).await?;
    }
    let root = validate_group_note_workspace_root(row.workspace, &row.group_owner_id)?;
    if root != prepared.root() {
        return Err(changed());
    }
    // This check must follow all awaits; an expired MCP lease cannot start I/O.
    check_note_write_guard(guard)?;
    prepared.publish()?;
    tx.rollback()
        .await
        .map_err(|_| ApiError::internal("failed to release group note publication transaction"))
}
