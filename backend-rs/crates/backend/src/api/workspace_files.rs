//! Shared, conversation-scoped workspace file access.
//!
//! Group and direct-chat file endpoints deliberately use the same service.  A
//! caller is authorised against the conversation row first, then against its
//! active local workspace, and every user supplied path is resolved below the
//! canonical workspace root before touching the filesystem.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tempfile::NamedTempFile;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    api::error::ApiError,
    api::AppState,
    runtime::group::{AttachmentKind, MessageAttachment},
    tools::{resolve_workspace_path, ToolError},
};

/// Maximum bytes retained for a text read/edit response and accepted on save.
/// This matches the native workspace `Read`/`Write` limits.
pub const MAX_WORKSPACE_TEXT_BYTES: usize = 1_000_000;
/// Maximum bytes sampled by the compatibility preview endpoint.
pub const MAX_WORKSPACE_PREVIEW_BYTES: usize = 64 * 1024;
/// Maximum characters returned by the compatibility preview endpoint.
pub const TEXT_WORKSPACE_PREVIEW_CHARS: usize = 20_000;
/// Maximum number of durable attachment references in one message.
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;

const BINARY_PREVIEW_MESSAGE: &str = "Preview is not available for binary or unsupported files.";
const PATH_ERROR_MESSAGE: &str =
    "workspace file paths must be relative and stay inside the conversation workspace";

static SAVE_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// The URL namespace of a conversation.  The database kind is intentionally
/// derived from this value instead of accepting a caller supplied kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationScope {
    Groups,
    DirectChats,
}

impl ConversationScope {
    pub const fn conversation_kind(self) -> &'static str {
        match self {
            Self::Groups => "group",
            Self::DirectChats => "direct",
        }
    }

    pub const fn route_segment(self) -> &'static str {
        match self {
            Self::Groups => "groups",
            Self::DirectChats => "direct-chats",
        }
    }
}

/// A canonical, owned, active local workspace bound to a conversation.
#[derive(Debug, Clone)]
pub struct OwnedLocalWorkspace {
    pub scope: ConversationScope,
    pub conversation_id: String,
    pub owner_id: String,
    pub workspace_id: String,
    pub root: PathBuf,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct WorkspaceFilePathQuery {
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceFileResponse {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<i64>,
    pub modified_at: Option<String>,
    pub abs_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceRootResponse {
    pub root: String,
    pub separator: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceFilePreviewResponse {
    pub path: String,
    pub name: String,
    pub is_text: bool,
    pub content: Option<String>,
    pub truncated: bool,
    pub message: Option<String>,
    pub size: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceFileTextResponse {
    pub path: String,
    pub name: String,
    pub mime_type: String,
    pub size: i64,
    pub content: Option<String>,
    pub is_text: bool,
    pub truncated: bool,
    pub version: String,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveWorkspaceFileTextRequest {
    pub content: String,
    pub version: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ConversationRow {
    owner_id: String,
    workspace_id: Option<String>,
    conversation_kind: String,
    status: String,
}

#[derive(Debug, sqlx::FromRow)]
struct WorkspaceRow {
    owner_id: String,
    backend_type: String,
    local_path: Option<String>,
    status: String,
}

#[derive(Debug)]
struct FileSnapshot {
    size: u64,
    digest: [u8; 32],
    captured: Vec<u8>,
    utf8_valid: bool,
    contains_nul: bool,
}

/// Load and validate the authenticated conversation's active local workspace.
pub async fn load_owned_local_workspace(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
) -> Result<OwnedLocalWorkspace, ApiError> {
    let conversation = sqlx::query_as::<_, ConversationRow>(
        "SELECT owner_id, workspace_id, conversation_kind, status FROM groups WHERE id = ?",
    )
    .bind(conversation_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("conversation not found"))?;

    if conversation.status != "active"
        || conversation.conversation_kind != scope.conversation_kind()
    {
        return Err(ApiError::not_found("conversation not found"));
    }
    if conversation.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "conversation belongs to another user",
        ));
    }

    let workspace_id = conversation
        .workspace_id
        .ok_or_else(|| ApiError::invalid_input("conversation has no bound workspace"))?;
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT owner_id, backend_type, local_path, status FROM workspaces WHERE id = ?",
    )
    .bind(&workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::invalid_input("conversation workspace is not active"))?;

    if workspace.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "conversation workspace belongs to another user",
        ));
    }
    if workspace.status != "active" {
        return Err(ApiError::invalid_input(
            "conversation workspace is not active",
        ));
    }
    if workspace.backend_type != "local" {
        return Err(ApiError::invalid_input(
            "conversation workspace requires a local backend",
        ));
    }
    let local_path = workspace
        .local_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::invalid_input("local workspace has no local_path"))?;
    let root = fs::canonicalize(local_path)
        .map_err(|_| ApiError::invalid_input("workspace path must be an existing directory"))?;
    if !root.is_dir() {
        return Err(ApiError::invalid_input(
            "workspace path must be an existing directory",
        ));
    }
    if root.to_str().is_none() {
        return Err(ApiError::invalid_input("workspace path is not valid UTF-8"));
    }

    Ok(OwnedLocalWorkspace {
        scope,
        conversation_id: conversation_id.to_string(),
        owner_id: owner_id.to_string(),
        workspace_id,
        root,
    })
}

/// Return the canonical workspace root for a conversation.
pub async fn workspace_root(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
) -> Result<WorkspaceRootResponse, ApiError> {
    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    Ok(WorkspaceRootResponse {
        root: path_to_utf8(&workspace.root)?,
        separator: std::path::MAIN_SEPARATOR.to_string(),
    })
}

/// List direct children of a workspace directory.  An empty `path` selects
/// the root; explicit `.` is rejected by the path validator.
pub async fn list_workspace_files(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
    relative: &str,
) -> Result<Vec<WorkspaceFileResponse>, ApiError> {
    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    let directory = resolve_workspace_directory(&workspace.root, relative)?;
    let mut rows = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|_| ApiError::invalid_input("workspace path is not a directory"))?
    {
        let entry = entry.map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        let entry_path = entry.path();
        let entry_name = entry.file_name();
        let name = entry_name
            .to_str()
            .ok_or_else(|| ApiError::invalid_input("workspace path is not valid UTF-8"))?;
        if name.starts_with('.') {
            continue;
        }
        rows.push(workspace_file_response(&entry_path, &workspace.root)?);
    }
    rows.sort_by(|left, right| {
        (if left.is_dir { 0 } else { 1 }, left.name.to_lowercase())
            .cmp(&(if right.is_dir { 0 } else { 1 }, right.name.to_lowercase()))
    });
    Ok(rows)
}

/// Compatibility preview response used by both conversation scopes.
pub async fn preview_workspace_file(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
    relative: &str,
) -> Result<WorkspaceFilePreviewResponse, ApiError> {
    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    let path = resolve_workspace_file(&workspace.root, relative)?;
    let mut file =
        File::open(&path).map_err(|_| ApiError::invalid_input("workspace path is not a file"))?;
    let size = file
        .metadata()
        .map_err(|_| ApiError::invalid_input("workspace path is not a file"))?
        .len();
    let mut sample = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_WORKSPACE_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut sample)
        .map_err(|_| ApiError::invalid_input("workspace file could not be read"))?;
    revalidate_resolved_file(&workspace.root, &path)?;
    let byte_truncated = sample.len() > MAX_WORKSPACE_PREVIEW_BYTES;
    let capped = &sample[..sample.len().min(MAX_WORKSPACE_PREVIEW_BYTES)];
    if !workspace_file_looks_text(&path, capped) {
        return Ok(WorkspaceFilePreviewResponse {
            path: display_workspace_path(&workspace.root, &path)?,
            name: workspace_file_name(&path)?,
            is_text: false,
            content: None,
            truncated: false,
            message: Some(BINARY_PREVIEW_MESSAGE.to_string()),
            size: Some(size_to_i64(size)?),
        });
    }
    let mut content = String::from_utf8_lossy(capped).to_string();
    let mut truncated = byte_truncated;
    if content.chars().count() > TEXT_WORKSPACE_PREVIEW_CHARS {
        content = content.chars().take(TEXT_WORKSPACE_PREVIEW_CHARS).collect();
        truncated = true;
    }
    Ok(WorkspaceFilePreviewResponse {
        path: display_workspace_path(&workspace.root, &path)?,
        name: workspace_file_name(&path)?,
        is_text: true,
        content: Some(content),
        truncated,
        message: None,
        size: Some(size_to_i64(size)?),
    })
}

/// Read a bounded UTF-8 text representation and hash the complete file.
pub async fn read_workspace_file_text(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
    relative: &str,
) -> Result<WorkspaceFileTextResponse, ApiError> {
    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    let path = resolve_workspace_file(&workspace.root, relative)?;
    let snapshot = read_validated_snapshot(&workspace.root, &path, MAX_WORKSPACE_TEXT_BYTES)?;
    text_response(&workspace.root, &path, snapshot)
}

/// Conditionally replace a UTF-8 workspace text file using a same-directory
/// temporary file and rename.  The caller's version is compared with a fresh
/// full-file SHA-256 immediately before writing.
pub async fn save_workspace_file_text(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
    relative: &str,
    content: &str,
    version: &str,
) -> Result<WorkspaceFileTextResponse, ApiError> {
    if content.len() > MAX_WORKSPACE_TEXT_BYTES {
        return Err(ApiError::invalid_input(format!(
            "workspace text must be at most {MAX_WORKSPACE_TEXT_BYTES} bytes"
        )));
    }
    if content.as_bytes().contains(&0) {
        return Err(ApiError::invalid_input(
            "workspace text must not contain NUL bytes",
        ));
    }
    if version.trim().is_empty() {
        return Err(ApiError::invalid_input("version is required"));
    }

    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    let _save_guard = SAVE_LOCK.lock().await;
    let path = resolve_workspace_file(&workspace.root, relative)?;
    let current = read_validated_snapshot(&workspace.root, &path, MAX_WORKSPACE_TEXT_BYTES)?;
    if current.size > MAX_WORKSPACE_TEXT_BYTES as u64 {
        return Err(ApiError::invalid_input(
            "workspace text is too large to edit",
        ));
    }
    let requested_version = version.trim();
    if digest_hex(&current.digest) != requested_version {
        return Err(ApiError::conflict(
            "workspace file changed since it was read",
        ));
    }
    if !current.utf8_valid || current.contains_nul {
        return Err(ApiError::invalid_input(
            "workspace file is not valid UTF-8 text",
        ));
    }

    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid_input("workspace path is invalid"))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    if !canonical_parent.starts_with(&workspace.root) {
        return Err(ApiError::invalid_input(
            "workspace file path escapes the workspace",
        ));
    }

    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|_| ApiError::internal("failed to create workspace temporary file"))?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|_| ApiError::internal("failed to write workspace temporary file"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| ApiError::internal("failed to flush workspace temporary file"))?;
    let permissions = fs::metadata(&path)
        .map_err(|_| ApiError::invalid_input("workspace file could not be read"))?
        .permissions();
    temporary
        .as_file()
        .set_permissions(permissions)
        .map_err(|_| ApiError::internal("failed to preserve workspace file permissions"))?;

    // Re-resolve and hash the complete file again immediately before rename.
    // The process-wide lock ensures competing API saves cannot both accept the
    // same version; this second check also catches ordinary external edits made
    // while the replacement was being prepared.
    let latest_path = resolve_workspace_file(&workspace.root, relative)?;
    if latest_path != path {
        return Err(ApiError::conflict(
            "workspace file changed since it was read",
        ));
    }
    let latest = read_validated_snapshot(&workspace.root, &latest_path, MAX_WORKSPACE_TEXT_BYTES)?;
    if latest.size > MAX_WORKSPACE_TEXT_BYTES as u64 {
        return Err(ApiError::invalid_input(
            "workspace text is too large to edit",
        ));
    }
    if !latest.utf8_valid || latest.contains_nul {
        return Err(ApiError::invalid_input(
            "workspace file is not valid UTF-8 text",
        ));
    }
    if digest_hex(&latest.digest) != requested_version {
        return Err(ApiError::conflict(
            "workspace file changed since it was read",
        ));
    }
    temporary
        .persist(&path)
        .map_err(|_| ApiError::internal("failed to replace workspace file"))?;

    let updated = read_validated_snapshot(&workspace.root, &path, MAX_WORKSPACE_TEXT_BYTES)?;
    text_response(&workspace.root, &path, updated)
}

/// Stream/download a file with MIME inferred from its actual path and a safe
/// ASCII `Content-Disposition` filename.
pub async fn stream_workspace_file(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
    relative: &str,
) -> Result<Response, ApiError> {
    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    let path = resolve_workspace_file(&workspace.root, relative)?;
    let bytes =
        fs::read(&path).map_err(|_| ApiError::invalid_input("workspace file could not be read"))?;
    revalidate_resolved_file(&workspace.root, &path)?;
    let filename = workspace_file_name(&path)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(workspace_file_content_type(&path)),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            header_safe_filename(&filename)
        ))
        .map_err(|_| ApiError::internal("failed to build download headers"))?,
    );
    Ok((headers, Body::from(bytes)).into_response())
}

/// Validate and materialise immutable attachment metadata for either
/// conversation scope.  `paths` are relative workspace paths supplied by the
/// message request; MIME and file kind are derived from the resolved file.
pub async fn validate_conversation_attachments(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
    paths: &[String],
) -> Result<Vec<MessageAttachment>, ApiError> {
    if paths.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(ApiError::invalid_input(format!(
            "at most {MAX_ATTACHMENTS_PER_MESSAGE} attachments are allowed"
        )));
    }
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let workspace = load_owned_local_workspace(pool, scope, conversation_id, owner_id).await?;
    let mut seen = HashSet::new();
    let mut attachments = Vec::with_capacity(paths.len());
    for relative in paths {
        let path = resolve_workspace_file(&workspace.root, relative)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        if !seen.insert(path.clone()) {
            return Err(ApiError::invalid_input("attachment paths must be unique"));
        }
        let metadata = fs::metadata(&path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        revalidate_resolved_file(&workspace.root, &path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        let relative_path = display_workspace_path(&workspace.root, &path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        let name = workspace_file_name(&path)
            .map_err(|_| ApiError::invalid_input("attachment path is invalid"))?;
        let mime_type = workspace_file_content_type(&path).to_string();
        let kind = match mime_type.as_str() {
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" => AttachmentKind::Image,
            _ => AttachmentKind::File,
        };
        attachments.push(MessageAttachment {
            id: Uuid::new_v4().to_string(),
            path: relative_path,
            name,
            mime_type,
            size: size_to_i64(metadata.len())?,
            kind,
        });
    }
    Ok(attachments)
}

fn text_response(
    root: &Path,
    path: &Path,
    snapshot: FileSnapshot,
) -> Result<WorkspaceFileTextResponse, ApiError> {
    let truncated = snapshot.size > MAX_WORKSPACE_TEXT_BYTES as u64;
    let is_text = snapshot.utf8_valid && !snapshot.contains_nul;
    let content = if is_text {
        let capped = &snapshot.captured[..snapshot.captured.len().min(MAX_WORKSPACE_TEXT_BYTES)];
        Some(utf8_prefix(capped))
    } else {
        None
    };
    Ok(WorkspaceFileTextResponse {
        path: display_workspace_path(root, path)?,
        name: workspace_file_name(path)?,
        mime_type: workspace_file_content_type(path).to_string(),
        size: size_to_i64(snapshot.size)?,
        content,
        is_text,
        truncated,
        version: digest_hex(&snapshot.digest),
        message: if is_text {
            None
        } else {
            Some(BINARY_PREVIEW_MESSAGE.to_string())
        },
    })
}

fn read_snapshot(path: &Path, capture_limit: usize) -> Result<FileSnapshot, ApiError> {
    let mut file = File::open(path).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => ApiError::not_found("workspace file not found"),
        _ => ApiError::invalid_input("workspace file could not be read"),
    })?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut captured = Vec::with_capacity(capture_limit.min(64 * 1024));
    let mut utf8_pending = Vec::new();
    let mut utf8_valid = true;
    let mut contains_nul = false;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| ApiError::invalid_input("workspace file could not be read"))?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        size = size.saturating_add(read as u64);
        contains_nul |= chunk.contains(&0);
        if captured.len() < capture_limit {
            let take = (capture_limit - captured.len()).min(read);
            captured.extend_from_slice(&chunk[..take]);
        }
        if utf8_valid {
            utf8_pending.extend_from_slice(chunk);
            match std::str::from_utf8(&utf8_pending) {
                Ok(_) => utf8_pending.clear(),
                Err(error) if error.error_len().is_none() => {
                    let valid_up_to = error.valid_up_to();
                    utf8_pending.drain(..valid_up_to);
                }
                Err(_) => utf8_valid = false,
            }
        }
    }
    if utf8_valid && !utf8_pending.is_empty() {
        utf8_valid = false;
    }
    let digest = hasher.finalize();
    let mut digest_bytes = [0_u8; 32];
    digest_bytes.copy_from_slice(&digest);
    Ok(FileSnapshot {
        size,
        digest: digest_bytes,
        captured,
        utf8_valid,
        contains_nul,
    })
}

fn read_validated_snapshot(
    root: &Path,
    path: &Path,
    capture_limit: usize,
) -> Result<FileSnapshot, ApiError> {
    let snapshot = read_snapshot(path, capture_limit)?;
    revalidate_resolved_file(root, path)?;
    Ok(snapshot)
}

fn resolve_workspace_directory(root: &Path, raw: &str) -> Result<PathBuf, ApiError> {
    let Some(relative) = normalize_relative_path(raw, true)? else {
        return Ok(root.to_path_buf());
    };
    let path = resolve_workspace_path(root, &relative).map_err(workspace_path_error)?;
    let canonical = fs::canonicalize(&path)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    ensure_inside_root(root, &canonical)?;
    if !canonical.is_dir() {
        return Err(ApiError::invalid_input("workspace path is not a directory"));
    }
    ensure_utf8_path(&canonical)?;
    Ok(canonical)
}

fn resolve_workspace_file(root: &Path, raw: &str) -> Result<PathBuf, ApiError> {
    let Some(relative) = normalize_relative_path(raw, false)? else {
        return Err(ApiError::invalid_input("workspace path is not a file"));
    };
    let path = resolve_workspace_path(root, &relative).map_err(workspace_path_error)?;
    let canonical = fs::canonicalize(&path).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => ApiError::not_found("workspace file not found"),
        _ => ApiError::invalid_input("workspace path is invalid"),
    })?;
    ensure_inside_root(root, &canonical)?;
    if !canonical.is_file() {
        return Err(ApiError::invalid_input("workspace path is not a file"));
    }
    ensure_utf8_path(&canonical)?;
    Ok(canonical)
}

fn normalize_relative_path(raw: &str, allow_empty: bool) -> Result<Option<String>, ApiError> {
    let normalized = raw.trim().replace('\\', "/");
    if normalized.is_empty() {
        if allow_empty {
            return Ok(None);
        }
        return Err(ApiError::invalid_input(PATH_ERROR_MESSAGE));
    }
    let mut chars = normalized.chars();
    if matches!((chars.next(), chars.next()), (Some(drive), Some(':')) if drive.is_ascii_alphabetic())
        || normalized.starts_with('/')
        || raw.starts_with('\\')
    {
        return Err(ApiError::invalid_input(PATH_ERROR_MESSAGE));
    }
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == ".." || part == "~")
    {
        return Err(ApiError::invalid_input(PATH_ERROR_MESSAGE));
    }
    Ok(Some(normalized))
}

fn workspace_file_response(path: &Path, root: &Path) -> Result<WorkspaceFileResponse, ApiError> {
    let canonical =
        fs::canonicalize(path).map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    ensure_inside_root(root, &canonical)?;
    ensure_utf8_path(path)?;
    let metadata =
        fs::metadata(path).map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|modified| OffsetDateTime::from(modified).format(&Rfc3339).ok());
    Ok(WorkspaceFileResponse {
        path: display_workspace_path(root, path)?,
        name: workspace_file_name(path)?,
        is_dir: metadata.is_dir(),
        size: if metadata.is_dir() {
            None
        } else {
            Some(size_to_i64(metadata.len())?)
        },
        modified_at,
        abs_path: path_to_utf8(path)?,
    })
}

fn display_workspace_path(root: &Path, path: &Path) -> Result<String, ApiError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    if relative.as_os_str().is_empty() {
        return Ok(String::new());
    }
    Ok(relative
        .to_str()
        .ok_or_else(|| ApiError::invalid_input("workspace path is not valid UTF-8"))?
        .replace('\\', "/"))
}

fn workspace_file_name(path: &Path) -> Result<String, ApiError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| ApiError::invalid_input("workspace path is not valid UTF-8"))
}

fn ensure_inside_root(root: &Path, path: &Path) -> Result<(), ApiError> {
    if !path.starts_with(root) {
        return Err(ApiError::invalid_input(
            "workspace file path escapes the conversation workspace",
        ));
    }
    Ok(())
}

fn revalidate_resolved_file(root: &Path, expected: &Path) -> Result<(), ApiError> {
    let metadata = fs::symlink_metadata(expected).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => ApiError::not_found("workspace file not found"),
        _ => ApiError::invalid_input("workspace path is invalid"),
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(ApiError::invalid_input(
            "workspace path is not a regular file",
        ));
    }
    let canonical = fs::canonicalize(expected)
        .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    ensure_inside_root(root, &canonical)?;
    ensure_utf8_path(&canonical)?;
    if canonical != expected {
        return Err(ApiError::invalid_input(
            "workspace file changed during access",
        ));
    }
    Ok(())
}

fn ensure_utf8_path(path: &Path) -> Result<(), ApiError> {
    if path.to_str().is_none() {
        return Err(ApiError::invalid_input("workspace path is not valid UTF-8"));
    }
    Ok(())
}

fn path_to_utf8(path: &Path) -> Result<String, ApiError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| ApiError::invalid_input("workspace path is not valid UTF-8"))
}

fn workspace_file_looks_text(path: &Path, sample: &[u8]) -> bool {
    if sample.contains(&0) {
        return false;
    }
    if workspace_file_has_text_extension(path) {
        return true;
    }
    std::str::from_utf8(sample).is_ok()
}

fn workspace_file_has_text_extension(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "json"
            | "jsonl"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "cfg"
            | "log"
            | "xml"
            | "html"
            | "htm"
            | "css"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "py"
            | "sh"
            | "bat"
            | "ps1"
            | "sql"
            | "rst"
    )
}

/// Infer a response MIME type from the resolved filesystem path.
pub fn workspace_file_content_type(path: &Path) -> &'static str {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return "application/octet-stream";
    };
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "log" | "csv" | "md" | "markdown" | "rst" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "jsx" => "text/javascript",
        "json" | "jsonl" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" | "ini" | "cfg" => "text/plain",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn utf8_prefix(bytes: &[u8]) -> String {
    let mut end = bytes.len();
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    String::from_utf8(bytes[..end].to_vec()).unwrap_or_default()
}

fn digest_hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn size_to_i64(size: u64) -> Result<i64, ApiError> {
    i64::try_from(size).map_err(|_| ApiError::invalid_input("workspace file is too large"))
}

fn header_safe_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|ch| {
            if ch.is_ascii() && ch != '"' && ch != '\\' && !ch.is_control() {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn workspace_path_error(error: ToolError) -> ApiError {
    match error {
        ToolError::Invalid(message) => {
            if message.contains("path must") || message.contains("workspace root") {
                ApiError::invalid_input(message)
            } else {
                ApiError::invalid_input(PATH_ERROR_MESSAGE)
            }
        }
        ToolError::Io(_) => ApiError::invalid_input("workspace path is invalid"),
    }
}

// Keep the shared handler contracts close to the service.  Scope-specific
// modules use these helpers as thin Axum adapters.
pub async fn list_handler(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: WorkspaceFilePathQuery,
) -> Result<Json<Vec<WorkspaceFileResponse>>, ApiError> {
    let owner_id = crate::api::auth::current_user_id(&headers, &state.auth.secret_key)?;
    let rows = list_workspace_files(
        state.db.pool(),
        scope,
        &conversation_id,
        &owner_id,
        &query.path,
    )
    .await?;
    Ok(Json(rows))
}

pub async fn root_handler(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
) -> Result<Json<WorkspaceRootResponse>, ApiError> {
    let owner_id = crate::api::auth::current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(
        workspace_root(state.db.pool(), scope, &conversation_id, &owner_id).await?,
    ))
}

pub async fn preview_handler(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: WorkspaceFilePathQuery,
) -> Result<Json<WorkspaceFilePreviewResponse>, ApiError> {
    let owner_id = crate::api::auth::current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(
        preview_workspace_file(
            state.db.pool(),
            scope,
            &conversation_id,
            &owner_id,
            &query.path,
        )
        .await?,
    ))
}

pub async fn download_handler(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: WorkspaceFilePathQuery,
) -> Result<Response, ApiError> {
    let owner_id = crate::api::auth::current_user_id(&headers, &state.auth.secret_key)?;
    stream_workspace_file(
        state.db.pool(),
        scope,
        &conversation_id,
        &owner_id,
        &query.path,
    )
    .await
}

pub async fn text_handler(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: WorkspaceFilePathQuery,
) -> Result<Json<WorkspaceFileTextResponse>, ApiError> {
    let owner_id = crate::api::auth::current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(
        read_workspace_file_text(
            state.db.pool(),
            scope,
            &conversation_id,
            &owner_id,
            &query.path,
        )
        .await?,
    ))
}

pub async fn save_text_handler(
    state: AppState,
    headers: HeaderMap,
    scope: ConversationScope,
    conversation_id: String,
    query: WorkspaceFilePathQuery,
    body: SaveWorkspaceFileTextRequest,
) -> Result<Json<WorkspaceFileTextResponse>, ApiError> {
    let owner_id = crate::api::auth::current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(
        save_workspace_file_text(
            state.db.pool(),
            scope,
            &conversation_id,
            &owner_id,
            &query.path,
            &body.content,
            &body.version,
        )
        .await?,
    ))
}
