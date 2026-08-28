//! Shared, conversation-scoped workspace file access.
//!
//! Group and direct-chat file endpoints deliberately use the same service.  A
//! caller is authorised against the conversation row first, then against its
//! active local workspace, and every user supplied path is resolved below the
//! canonical workspace root before touching the filesystem.
//!
//! A conversation can expose more than one root.  [`ConversationRoot`] names
//! which one a request addresses: the group or direct-chat workspace by
//! default, or a member agent's own workspace when `agent_id` is
//! given. An agent working elsewhere would otherwise write files that appear
//! nowhere in the UI.

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
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tempfile::NamedTempFile;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    api::error::ApiError,
    runtime::group::{AttachmentKind, MessageAttachment},
    runtime::workspace_scope::WorkspaceMode,
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
const MAX_WORKSPACE_SEARCH_QUERY_BYTES: usize = 2 * 1024;
// ponytail: bounded search output; paginate if 2,000 results becomes limiting.
const MAX_WORKSPACE_SEARCH_RESULTS: usize = 2_000;

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

/// Which root inside a conversation a request addresses.
///
/// `agent_id` is `None` for the default root: the group workspace for groups or
/// the direct agent's live workspace for direct chats. `Some(id)` addresses a
/// member agent's additional workspace.
#[derive(Debug, Clone, Copy)]
pub struct ConversationRoot<'a> {
    pub scope: ConversationScope,
    pub conversation_id: &'a str,
    pub owner_id: &'a str,
    pub agent_id: Option<&'a str>,
}

impl<'a> ConversationRoot<'a> {
    /// Address the conversation's default workspace.
    pub fn conversation(
        scope: ConversationScope,
        conversation_id: &'a str,
        owner_id: &'a str,
    ) -> Self {
        Self {
            scope,
            conversation_id,
            owner_id,
            agent_id: None,
        }
    }

    /// Address the root named by a request query, which may select an agent.
    pub fn from_query(
        scope: ConversationScope,
        conversation_id: &'a str,
        owner_id: &'a str,
        agent_id: Option<&'a str>,
    ) -> Self {
        Self {
            scope,
            conversation_id,
            owner_id,
            agent_id,
        }
    }
}

/// A canonical, owned, active local workspace addressed inside a conversation.
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
    /// Address this member agent's own workspace instead of the conversation's.
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub show_hidden: bool,
    #[serde(default)]
    pub search: Option<String>,
}

impl WorkspaceFilePathQuery {
    /// The agent selector, treating blank as absent.
    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn search(&self) -> Option<&str> {
        self.search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

/// One browsable root inside a conversation.
#[derive(Debug, Serialize, Clone)]
pub struct ConversationRootEntry {
    /// `None` marks the conversation's default workspace.
    pub agent_id: Option<String>,
    pub display_name: Option<String>,
    /// The agent's workspace mode; `None` for the conversation entry.
    pub workspace_mode: Option<String>,
    pub workspace_id: String,
    pub name: String,
    pub root: String,
    /// Whether an agent's plain relative paths resolve here. True for the
    /// default entry, and for an agent that works only in its own folder;
    /// false for a folder the agent merely has mounted.
    pub is_primary: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceFileResponse {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub ignored: bool,
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

/// Load and validate the active local workspace a request addresses.
pub async fn load_owned_local_workspace(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
) -> Result<OwnedLocalWorkspace, ApiError> {
    let conversation = load_owned_conversation(pool, target).await?;
    let workspace_id = match target.agent_id {
        Some(agent_id) => member_agent_workspace_id(pool, target, agent_id).await?,
        None => conversation
            .workspace_id
            .ok_or_else(|| ApiError::invalid_input("conversation has no bound workspace"))?,
    };
    let root = load_local_workspace_root(pool, &workspace_id, target.owner_id).await?;

    Ok(OwnedLocalWorkspace {
        scope: target.scope,
        conversation_id: target.conversation_id.to_string(),
        owner_id: target.owner_id.to_string(),
        workspace_id,
        root,
    })
}

/// Load the conversation row, checking ownership, status, and that its kind
/// matches the URL namespace so one kind cannot be reached through the other's
/// routes.
async fn load_owned_conversation(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
) -> Result<ConversationRow, ApiError> {
    let conversation = sqlx::query_as::<_, ConversationRow>(
        "SELECT owner_id, workspace_id, conversation_kind, status FROM groups WHERE id = ?",
    )
    .bind(target.conversation_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("conversation not found"))?;

    if conversation.status != "active"
        || conversation.conversation_kind != target.scope.conversation_kind()
    {
        return Err(ApiError::not_found("conversation not found"));
    }
    if conversation.owner_id != target.owner_id {
        return Err(ApiError::permission_denied(
            "conversation belongs to another user",
        ));
    }
    Ok(conversation)
}

/// The workspace bound to an agent that is an active member of this
/// conversation. Membership is required: owning an agent is not a licence to
/// browse its folder through an unrelated conversation's routes.
async fn member_agent_workspace_id(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
    agent_id: &str,
) -> Result<String, ApiError> {
    let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT a.workspace_id, ga.context_scope_json FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? AND ga.agent_id = ? AND ga.status = 'active' \
           AND a.status = 'active' AND a.owner_id = ?",
    )
    .bind(target.conversation_id)
    .bind(agent_id)
    .bind(target.owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?
    .ok_or_else(|| ApiError::not_found("agent is not a member of this conversation"))?;

    let (workspace_id, context_scope_json) = row;
    if !matches!(
        WorkspaceMode::from_context_scope(context_scope_json.as_deref()),
        WorkspaceMode::SelfOnly | WorkspaceMode::GroupAndSelf
    ) {
        return Err(ApiError::permission_denied(
            "agent workspace is not shared with this conversation",
        ));
    }
    workspace_id.ok_or_else(|| ApiError::invalid_input("agent has no bound workspace"))
}

/// Validate an owned, active, local workspace and canonicalize its root.
async fn load_local_workspace_root(
    pool: &SqlitePool,
    workspace_id: &str,
    owner_id: &str,
) -> Result<PathBuf, ApiError> {
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT owner_id, backend_type, local_path, status FROM workspaces WHERE id = ?",
    )
    .bind(workspace_id)
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
    Ok(root)
}

/// List every root a viewer may browse for this conversation: its own
/// workspace, plus each active member agent whose own workspace is actually
/// reachable during that agent's turns (mode `self` or `group_and_self`) and is
/// a different workspace from the conversation's.
///
/// Roots that fail to resolve are omitted rather than failing the request — the
/// switcher should still list what it can offer.
pub async fn list_conversation_roots(
    pool: &SqlitePool,
    scope: ConversationScope,
    conversation_id: &str,
    owner_id: &str,
) -> Result<Vec<ConversationRootEntry>, ApiError> {
    let target = ConversationRoot::conversation(scope, conversation_id, owner_id);
    let conversation = load_owned_conversation(pool, target).await?;
    let mut entries = Vec::new();

    let conversation_workspace_id = conversation.workspace_id.clone();
    if let Some(workspace_id) = conversation_workspace_id.as_deref() {
        if let Some(entry) =
            root_entry(pool, workspace_id, owner_id, None, None, None, true).await?
        {
            entries.push(entry);
        }
    }

    if scope == ConversationScope::DirectChats {
        return Ok(entries);
    }

    let members = sqlx::query_as::<_, MemberAgentRow>(
        "SELECT ga.agent_id, COALESCE(ga.display_name, a.name) AS display_name, \
                ga.context_scope_json, a.workspace_id \
         FROM group_agents ga JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? AND ga.status = 'active' AND a.status = 'active' \
           AND a.owner_id = ? \
         ORDER BY ga.joined_at ASC, ga.agent_id ASC",
    )
    .bind(conversation_id)
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    for member in members {
        let Some(workspace_id) = member.workspace_id.as_deref() else {
            continue;
        };
        if Some(workspace_id) == conversation_workspace_id.as_deref() {
            continue;
        }
        let mode = WorkspaceMode::from_context_scope(member.context_scope_json.as_deref());
        if !matches!(mode, WorkspaceMode::SelfOnly | WorkspaceMode::GroupAndSelf) {
            continue;
        }
        if let Some(entry) = root_entry(
            pool,
            workspace_id,
            owner_id,
            Some(member.agent_id),
            Some(member.display_name),
            Some(mode.as_str().to_string()),
            matches!(mode, WorkspaceMode::SelfOnly),
        )
        .await?
        {
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Build one root entry, or `None` when the workspace cannot be browsed.
#[allow(clippy::too_many_arguments)]
async fn root_entry(
    pool: &SqlitePool,
    workspace_id: &str,
    owner_id: &str,
    agent_id: Option<String>,
    display_name: Option<String>,
    workspace_mode: Option<String>,
    is_primary: bool,
) -> Result<Option<ConversationRootEntry>, ApiError> {
    let Ok(root) = load_local_workspace_root(pool, workspace_id, owner_id).await else {
        return Ok(None);
    };
    let name = sqlx::query_scalar::<_, String>("SELECT name FROM workspaces WHERE id = ?")
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))?
        .unwrap_or_default();
    Ok(Some(ConversationRootEntry {
        agent_id,
        display_name,
        workspace_mode,
        workspace_id: workspace_id.to_string(),
        name,
        root: path_to_utf8(&root)?,
        is_primary,
    }))
}

/// Return the canonical workspace root for a conversation.
pub async fn workspace_root(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
) -> Result<WorkspaceRootResponse, ApiError> {
    let workspace = load_owned_local_workspace(pool, target).await?;
    Ok(WorkspaceRootResponse {
        root: path_to_utf8(&workspace.root)?,
        separator: std::path::MAIN_SEPARATOR.to_string(),
    })
}

/// List direct children of a workspace directory.  An empty `path` selects
/// the root; explicit `.` is rejected by the path validator.
pub async fn list_workspace_files(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
    relative: &str,
    show_hidden: bool,
    search: Option<&str>,
) -> Result<Vec<WorkspaceFileResponse>, ApiError> {
    let workspace = load_owned_local_workspace(pool, target).await?;
    let directory = resolve_workspace_directory(&workspace.root, relative)?;
    let mut rows = if let Some(search) = search {
        let root = workspace.root.clone();
        let search = search.to_string();
        tokio::task::spawn_blocking(move || {
            search_workspace_files(&root, &directory, &search, show_hidden)
        })
        .await
        .map_err(|_| ApiError::internal("workspace file search failed"))??
    } else {
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
            let metadata = fs::symlink_metadata(&entry_path)
                .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
            if !show_hidden && is_hidden_entry(name, &metadata) {
                continue;
            }
            rows.push(workspace_file_response(
                &entry_path,
                &workspace.root,
                metadata,
            )?);
        }
        rows.sort_by_cached_key(|row| (if row.is_dir { 0 } else { 1 }, row.name.to_lowercase()));
        rows
    };
    let paths = rows.iter().map(|row| row.path.clone()).collect::<Vec<_>>();
    let ignored = crate::git::ignored_paths(&workspace.root, &paths).await;
    for row in &mut rows {
        row.ignored = ignored.contains(&row.path);
    }
    Ok(rows)
}

fn search_workspace_files(
    root: &Path,
    directory: &Path,
    search: &str,
    show_hidden: bool,
) -> Result<Vec<WorkspaceFileResponse>, ApiError> {
    if search.len() > MAX_WORKSPACE_SEARCH_QUERY_BYTES {
        return Err(ApiError::invalid_input("workspace file search is too long"));
    }
    let tokens = search
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let entries = WalkDir::new(directory)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if show_hidden || entry.depth() == 0 {
                return true;
            }
            let Some(name) = entry.file_name().to_str() else {
                return true;
            };
            fs::symlink_metadata(entry.path())
                .map(|metadata| !is_hidden_entry(name, &metadata))
                .unwrap_or(true)
        });
    for entry in entries {
        let entry = entry.map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        let row = workspace_file_response(entry.path(), root, metadata)?;
        let path = row.path.to_lowercase();
        if tokens.iter().all(|token| path.contains(token)) {
            rows.push(row);
            if rows.len() >= MAX_WORKSPACE_SEARCH_RESULTS {
                break;
            }
        }
    }
    rows.sort_by_cached_key(|row| (if row.is_dir { 0 } else { 1 }, row.path.to_lowercase()));
    Ok(rows)
}

fn is_hidden_entry(name: &str, metadata: &fs::Metadata) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

/// Compatibility preview response used by both conversation scopes.
pub async fn preview_workspace_file(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
    relative: &str,
) -> Result<WorkspaceFilePreviewResponse, ApiError> {
    let workspace = load_owned_local_workspace(pool, target).await?;
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
    target: ConversationRoot<'_>,
    relative: &str,
) -> Result<WorkspaceFileTextResponse, ApiError> {
    let workspace = load_owned_local_workspace(pool, target).await?;
    let path = resolve_workspace_file(&workspace.root, relative)?;
    let snapshot = read_validated_snapshot(&workspace.root, &path, MAX_WORKSPACE_TEXT_BYTES)?;
    text_response(&workspace.root, &path, snapshot)
}

/// Conditionally replace a UTF-8 workspace text file using a same-directory
/// temporary file and rename.  The caller's version is compared with a fresh
/// full-file SHA-256 immediately before writing.
pub async fn save_workspace_file_text(
    pool: &SqlitePool,
    target: ConversationRoot<'_>,
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

    let workspace = load_owned_local_workspace(pool, target).await?;
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
    target: ConversationRoot<'_>,
    relative: &str,
) -> Result<Response, ApiError> {
    let workspace = load_owned_local_workspace(pool, target).await?;
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
    target: ConversationRoot<'_>,
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
    let workspace = load_owned_local_workspace(pool, target).await?;
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

fn workspace_file_response(
    path: &Path,
    root: &Path,
    mut metadata: fs::Metadata,
) -> Result<WorkspaceFileResponse, ApiError> {
    if metadata_is_link_or_reparse(&metadata) {
        let canonical = fs::canonicalize(path)
            .map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
        ensure_inside_root(root, &canonical)?;
        metadata =
            fs::metadata(path).map_err(|_| ApiError::invalid_input("workspace path is invalid"))?;
    }
    ensure_utf8_path(path)?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|modified| OffsetDateTime::from(modified).format(&Rfc3339).ok());
    Ok(WorkspaceFileResponse {
        path: display_workspace_path(root, path)?,
        name: workspace_file_name(path)?,
        is_dir: metadata.is_dir(),
        ignored: false,
        size: if metadata.is_dir() {
            None
        } else {
            Some(size_to_i64(metadata.len())?)
        },
        modified_at,
        abs_path: path_to_utf8(path)?,
    })
}

fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
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

/// Row backing one member agent in the root listing.
#[derive(Debug, sqlx::FromRow)]
struct MemberAgentRow {
    agent_id: String,
    display_name: String,
    context_scope_json: Option<String>,
    workspace_id: Option<String>,
}
