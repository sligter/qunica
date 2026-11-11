//! Shared Notes commands for HTTP, ACP and approved application actions.
//!
//! Note: SQL commit, content publication and derived-index refresh retain their
//! existing order. Accepted receipts do not imply a command is safe to replay.
//! See specs/notes-failure-contract.md for the remaining recovery boundaries.
use super::{
    access::{
        check_note_write_guard, group_notes_workspace_root, load_active_note_group,
        GroupNoteWriteGuard,
    },
    files::{
        self as note_files, delete_group_note_content, group_note_path, inspect_group_note_file,
        publish_group_note_create, read_group_note_content, stage_group_note_create, NOTES_DIR,
        NOTE_FILE_SUFFIX,
    },
    note_update,
};
use crate::api::error::ApiError;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{fs, io, path::Path as FsPath};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

pub(crate) const GROUP_NOTE_COLUMNS: &str = "id, group_id, title, content, created_at, updated_at";
const NOTE_INDEX_FILE: &str = "index.md";

#[derive(Debug, Deserialize)]
pub struct GroupNoteCreateRequest {
    title: String,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupNoteUpdateRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupNoteResponse {
    id: String,
    group_id: String,
    title: String,
    content: String,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<GroupNoteWarning>,
}

#[derive(Debug, Serialize)]
struct GroupNoteWarning {
    code: &'static str,
    reason: String,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct GroupNoteRow {
    pub(crate) id: String,
    pub(crate) group_id: String,
    pub(crate) title: String,
    pub(crate) content: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl GroupNoteRow {
    fn into_response_with_content(self, content: String) -> GroupNoteResponse {
        GroupNoteResponse {
            id: self.id,
            group_id: self.group_id,
            title: self.title,
            content,
            created_at: self.created_at,
            updated_at: self.updated_at,
            warning: None,
        }
    }
}

pub(crate) async fn list_group_notes(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
) -> Result<Vec<GroupNoteResponse>, ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;

    let group = load_active_note_group(pool, &group_id, owner_id).await?;
    group_notes_workspace_root(pool, &group, &group.owner_id).await?;
    let rows = fetch_group_note_rows(pool, &group_id).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let content = row.content.clone();
            row.into_response_with_content(content)
        })
        .collect())
}

pub(crate) async fn get_group_note_inner(
    pool: &SqlitePool,
    user_id: &str,
    group_id: &str,
    note_id: &str,
) -> Result<GroupNoteResponse, ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let note_id = validate_uuid(note_id, "note id")?;
    let group = load_active_note_group(pool, &group_id, user_id).await?;
    let root = group_notes_workspace_root(pool, &group, &group.owner_id).await?;
    let row = load_active_group_note(pool, &group_id, &note_id).await?;
    let content = read_group_note_content(&root, &row.id, &row.content)?;
    Ok(row.into_response_with_content(content))
}

pub(crate) async fn get_group_note_by_id_inner(
    pool: &SqlitePool,
    owner_id: &str,
    note_id: &str,
) -> Result<GroupNoteResponse, ApiError> {
    let note_id = validate_uuid(note_id, "note id")?;
    let group_id = owned_group_id_for_note(pool, owner_id, &note_id).await?;
    get_group_note_inner(pool, owner_id, &group_id, &note_id).await
}

pub(crate) async fn create_group_note_with_pool(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
    body: GroupNoteCreateRequest,
    guard: Option<GroupNoteWriteGuard<'_>>,
) -> Result<GroupNoteResponse, ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let group = load_active_note_group(pool, &group_id, owner_id).await?;
    let root = group_notes_workspace_root(pool, &group, &group.owner_id).await?;
    let title = validate_note_title(&body.title)?;
    let note_id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let content = body
        .content
        .filter(|content| !content.trim().is_empty())
        .unwrap_or_else(|| crate::group_notes::default_content(&title, &now[..10]));

    let mut tx = crate::db::begin_write(pool)
        .await
        .map_err(|_| ApiError::internal("failed to start group note create transaction"))?;

    check_note_write_guard(guard)?;
    sqlx::query(
        "INSERT INTO group_notes \
         (id, group_id, author_id, title, content, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&note_id)
    .bind(&group_id)
    .bind(owner_id)
    .bind(&title)
    .bind(&content)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create group note"))?;

    check_note_write_guard(guard)?;
    // Note: publish only after COMMIT. Rollback must not leave an unregistered
    // note or delete a target that another writer may have created meanwhile.
    let staged = stage_group_note_create(&root, &note_id, &content)?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group note create"))?;
    // A scoped MCP dispatch can expire while COMMIT awaits. Registration is
    // already durable, but a revoked lease must not start a new file publication.
    let publication = check_note_write_guard(guard)
        .and_then(|()| publish_group_note_create(&root, &note_id, staged));
    // Stop filesystem work if publication failed. Refresh the index only after
    // successful publication; retain identity without inviting callers to retry.
    let warning = match publication {
        Err(error) => {
            // Note: return the accepted INSERT, just as update publication does.
            // Another read may be unavailable or see a later deletion; neither
            // makes this committed command safe to replay. No index I/O follows.
            let mut response = GroupNoteRow {
                id: note_id,
                group_id,
                title,
                content: content.clone(),
                created_at: now.clone(),
                updated_at: now,
            }
            .into_response_with_content(content);
            response.warning = Some(GroupNoteWarning {
                code: "note_content_pending",
                reason: error.message_text(),
            });
            return Ok(response);
        }
        Ok(()) => sync_group_note_index(pool, &root, &group_id)
            .await
            .err()
            .map(|error| GroupNoteWarning {
                code: "note_index_pending",
                reason: error.message_text(),
            }),
    };

    let row = fetch_group_note_row(pool, &group_id, &note_id)
        .await?
        .ok_or_else(|| ApiError::internal("group note vanished after insert"))?;
    let content = read_group_note_content(&root, &row.id, &row.content)?;
    let mut response = row.into_response_with_content(content);
    response.warning = warning;
    Ok(response)
}

pub(crate) async fn update_group_note_with_pool(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
    note_id: &str,
    body: GroupNoteUpdateRequest,
    guard: Option<GroupNoteWriteGuard<'_>>,
) -> Result<GroupNoteResponse, ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let note_id = validate_uuid(note_id, "note id")?;
    let group = load_active_note_group(pool, &group_id, owner_id).await?;
    let root = group_notes_workspace_root(pool, &group, &group.owner_id).await?;
    let existing = load_active_group_note(pool, &group_id, &note_id).await?;

    let title = match body.title.as_deref() {
        Some(raw) => validate_note_title(raw)?,
        None => existing.title,
    };
    let should_write_content = body.content.is_some();
    let content = body.content.unwrap_or(existing.content);
    let now = now_rfc3339();

    let mut tx = crate::db::begin_write(pool)
        .await
        .map_err(|_| ApiError::internal("failed to start group note update transaction"))?;

    check_note_write_guard(guard)?;
    // Prepare without changing the target: a rejected COMMIT must leave both
    // the previous file and any independent edits untouched.
    let prepared = should_write_content
        .then(|| note_files::prepare(&root, &note_id, &content))
        .transpose()?;
    if let Some(expected) = guard.and_then(|g| g.expected_content) {
        // Note: MCP validates an edit before writing. Recheck under the same
        // SQLite writer transaction used by UI updates so a slow model check
        // cannot silently overwrite a newer edit.
        let current: Option<String> = sqlx::query_scalar(
            "SELECT content FROM group_notes WHERE id = ? AND group_id = ? AND status = 'active'",
        )
        .bind(&note_id)
        .bind(&group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to check group note revision"))?;
        let current = current.ok_or_else(|| ApiError::not_found("group note no longer exists"))?;
        if read_group_note_content(&root, &note_id, &current)? != expected {
            return Err(ApiError::conflict(
                "group note changed; read it again before editing",
            ));
        }
        if let Some(prepared) = &prepared {
            prepared.check_expected(expected)?;
        }
    }

    let updated = sqlx::query(
        "UPDATE group_notes SET title = ?, content = ?, updated_at = ? \
         WHERE id = ? AND group_id = ? AND status = 'active'",
    )
    .bind(&title)
    .bind(&content)
    .bind(&now)
    .bind(&note_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update group note"))?;
    if updated.rows_affected() != 1 {
        return Err(ApiError::not_found("group note no longer exists"));
    }

    check_note_write_guard(guard)?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group note update"))?;
    if let Some(prepared) = prepared {
        if let Err(error) = note_update::publish(
            pool,
            owner_id,
            &group_id,
            &group.owner_id,
            group.workspace_id.as_deref(),
            prepared,
            guard,
        )
        .await
        {
            // Note: registration is committed even if publication or a later
            // read is unavailable. Keep the accepted receipt; do not write the
            // index or invite callers to replay an already committed mutation.
            let mut response = GroupNoteRow {
                id: note_id,
                group_id,
                title,
                content: content.clone(),
                created_at: existing.created_at,
                updated_at: now,
            }
            .into_response_with_content(content);
            response.warning = Some(GroupNoteWarning {
                code: "note_content_pending",
                reason: error.message_text(),
            });
            return Ok(response);
        }
    }
    let warning = sync_group_note_index(pool, &root, &group_id)
        .await
        .err()
        .map(|error| GroupNoteWarning {
            code: "note_index_pending",
            reason: error.message_text(),
        });

    let row = fetch_group_note_row(pool, &group_id, &note_id)
        .await?
        .ok_or_else(|| ApiError::internal("group note vanished after update"))?;
    let content = read_group_note_content(&root, &row.id, &row.content)?;
    let mut response = row.into_response_with_content(content);
    response.warning = warning;
    Ok(response)
}

pub(crate) async fn update_group_note_by_id_inner(
    pool: &SqlitePool,
    owner_id: &str,
    note_id: &str,
    body: GroupNoteUpdateRequest,
) -> Result<GroupNoteResponse, ApiError> {
    let note_id = validate_uuid(note_id, "note id")?;
    let group_id = owned_group_id_for_note(pool, owner_id, &note_id).await?;
    update_group_note_with_pool(pool, owner_id, &group_id, &note_id, body, None).await
}

pub(crate) async fn delete_group_note(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
    note_id: &str,
) -> Result<(), ApiError> {
    let group_id = validate_uuid(group_id, "group id")?;
    let note_id = validate_uuid(note_id, "note id")?;

    let group = load_active_note_group(pool, &group_id, owner_id).await?;
    let root = group_notes_workspace_root(pool, &group, &group.owner_id).await?;
    load_active_group_note(pool, &group_id, &note_id).await?;

    let now = now_rfc3339();
    let mut tx = crate::db::begin_write(pool)
        .await
        .map_err(|_| ApiError::internal("failed to start group note delete transaction"))?;

    sqlx::query(
        "UPDATE group_notes SET status = 'deleted', updated_at = ? \
         WHERE id = ? AND group_id = ? AND status = 'active'",
    )
    .bind(&now)
    .bind(&note_id)
    .bind(&group_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to delete group note"))?;

    // Note: native note edits may exist only in the file. Validate first, but
    // never unlink before COMMIT: rollback cannot recover those newer bytes.
    group_note_path(&root, &note_id)?;
    inspect_group_note_file(&root, &note_id)?;

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group note delete"))?;
    delete_group_note_content(&root, &note_id).map_err(|error| {
        // DELETE 500 responses are retried by the existing API client. Report
        // the committed identity as a conflict instead of replaying cleanup.
        ApiError::new(
            StatusCode::CONFLICT,
            "note_cleanup_pending",
            "group note was deleted but its file cleanup failed",
        )
        .with_details(serde_json::json!({
            "committed": true,
            "group_id": group_id,
            "note_id": note_id,
            "phase": "file_cleanup",
            "reason": error.message_text(),
        }))
    })?;
    sync_group_note_index(pool, &root, &group_id)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::CONFLICT,
                "note_index_pending",
                "group note was deleted but its index could not be refreshed",
            )
            .with_details(serde_json::json!({
                "committed": true,
                "group_id": group_id,
                "note_id": note_id,
                "phase": "index_sync",
                "reason": error.message_text(),
            }))
        })?;

    Ok(())
}

async fn fetch_group_note_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupNoteRow>, ApiError> {
    let sql = "SELECT id, group_id, title, '' AS content, created_at, updated_at FROM group_notes \
         WHERE group_id = ? AND status = 'active' \
         ORDER BY updated_at DESC, id DESC";
    sqlx::query_as::<_, GroupNoteRow>(sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_note_row(
    pool: &SqlitePool,
    group_id: &str,
    note_id: &str,
) -> Result<Option<GroupNoteRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_NOTE_COLUMNS} FROM group_notes \
         WHERE group_id = ? AND id = ? AND status = 'active'"
    );
    sqlx::query_as::<_, GroupNoteRow>(&sql)
        .bind(group_id)
        .bind(note_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn load_active_group_note(
    pool: &SqlitePool,
    group_id: &str,
    note_id: &str,
) -> Result<GroupNoteRow, ApiError> {
    fetch_group_note_row(pool, group_id, note_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group note not found"))
}

async fn owned_group_id_for_note(
    pool: &SqlitePool,
    owner_id: &str,
    note_id: &str,
) -> Result<String, ApiError> {
    sqlx::query_scalar(
        "SELECT n.group_id FROM group_notes n \
         JOIN groups g ON g.id = n.group_id \
         WHERE n.id = ? AND n.status = 'active' AND g.owner_id = ? \
           AND g.status = 'active' AND g.conversation_kind = 'group'",
    )
    .bind(note_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("group note not found"))
}

pub(crate) fn validate_note_title(raw: &str) -> Result<String, ApiError> {
    // A title is the first Markdown header line of the note, so it must stay single-line.
    let title = raw.replace(['\r', '\n'], " ").trim().to_string();
    let len = title.chars().count();
    if !(1..=200).contains(&len) {
        return Err(ApiError::invalid_input(
            "title must be between 1 and 200 characters",
        ));
    }
    Ok(title)
}

async fn sync_group_note_index(
    pool: &SqlitePool,
    root: &FsPath,
    group_id: &str,
) -> Result<(), ApiError> {
    let rows = fetch_group_note_rows(pool, group_id).await?;
    let notes_dir = root.join(NOTES_DIR);
    fs::create_dir_all(&notes_dir)
        .map_err(|_| ApiError::invalid_input("group notes path is not a directory"))?;
    let path = notes_dir.join(NOTE_INDEX_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(ApiError::invalid_input(
                "group note index path is not a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(ApiError::invalid_input("group note index path is invalid")),
    }
    let mut index = String::from("# Group notes\n");
    for row in rows {
        let title = row
            .title
            .replace(['\r', '\n'], " ")
            .replace('\\', "\\\\")
            .replace('[', "\\[")
            .replace(']', "\\]");
        index.push_str(&format!("\n- [{title}]({}{})", row.id, NOTE_FILE_SUFFIX));
    }
    index.push('\n');
    fs::write(path, index).map_err(|_| ApiError::internal("failed to write group note index"))
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
