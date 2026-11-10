//! Shared Notes workspace, membership and dispatch-lease checks.
//!
//! Callers retain the original transaction and check order. In particular,
//! publication must check the lease after its last awaited authorization read.
//! See specs/notes-failure-contract.md for ownership and failure semantics.
use crate::{
    api::error::ApiError,
    db::groups::{fetch_row, GroupRow},
};
use sqlx::{Sqlite, SqlitePool};
use std::{fs, path::PathBuf};

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct GroupNoteWorkspaceRow {
    pub(crate) owner_id: String,
    pub(crate) backend_type: String,
    pub(crate) local_path: Option<String>,
    pub(crate) status: String,
}

/// A scoped MCP write can expire while waiting for SQLite or model validation.
#[derive(Clone, Copy)]
pub(crate) struct GroupNoteWriteGuard<'a> {
    pub active: &'a std::sync::atomic::AtomicBool,
    pub expected_content: Option<&'a str>,
}

pub(crate) fn check_note_write_guard(
    guard: Option<GroupNoteWriteGuard<'_>>,
) -> Result<(), ApiError> {
    if guard.is_some_and(|g| !g.active.load(std::sync::atomic::Ordering::SeqCst)) {
        return Err(ApiError::permission_denied(
            "group notes access has expired",
        ));
    }
    Ok(())
}

pub(crate) async fn group_notes_root_for_owner(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
) -> Result<PathBuf, ApiError> {
    let group = load_active_note_group(pool, group_id, owner_id).await?;
    group_notes_workspace_root(pool, &group, owner_id).await
}

pub(crate) async fn group_notes_workspace_root(
    pool: &SqlitePool,
    group: &GroupRow,
    owner_id: &str,
) -> Result<PathBuf, ApiError> {
    let workspace_id = group
        .workspace_id
        .as_deref()
        .ok_or_else(|| ApiError::invalid_input("group has no bound workspace"))?;
    let row = sqlx::query_as::<_, GroupNoteWorkspaceRow>(
        "SELECT owner_id, backend_type, local_path, status FROM workspaces WHERE id = ?",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::invalid_input("group workspace is not active"))?;

    validate_group_note_workspace_root(row, owner_id)
}

pub(crate) fn validate_group_note_workspace_root(
    row: GroupNoteWorkspaceRow,
    owner_id: &str,
) -> Result<PathBuf, ApiError> {
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "group workspace belongs to another user",
        ));
    }
    if row.status != "active" {
        return Err(ApiError::invalid_input("group workspace is not active"));
    }
    if row.backend_type != "local" {
        return Err(ApiError::invalid_input(
            "group notes require a local workspace",
        ));
    }
    let local_path = row
        .local_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| ApiError::invalid_input("local workspace has no local_path"))?;
    let root = fs::canonicalize(local_path).map_err(|_| {
        ApiError::invalid_input("group workspace path must be an existing directory")
    })?;
    if !root.is_dir() {
        return Err(ApiError::invalid_input(
            "group workspace path must be an existing directory",
        ));
    }
    Ok(root)
}

pub(crate) async fn load_active_note_group(
    pool: &SqlitePool,
    group_id: &str,
    user_id: &str,
) -> Result<GroupRow, ApiError> {
    let group = fetch_row(pool, group_id)
        .await?
        .filter(|row| row.status == "active")
        .ok_or_else(|| ApiError::not_found("group not found"))?;
    if group.owner_id == user_id {
        return Ok(group);
    }
    check_group_note_membership(pool, group_id, user_id).await?;
    Ok(group)
}

pub(crate) async fn check_group_note_membership<'e>(
    executor: impl sqlx::Executor<'e, Database = Sqlite>,
    group_id: &str,
    user_id: &str,
) -> Result<(), ApiError> {
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM group_members \
         WHERE group_id = ? AND user_id = ? AND status = 'active')",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_one(executor)
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    if !member {
        return Err(ApiError::permission_denied(
            "group notes are not shared with this user",
        ));
    }
    Ok(())
}
