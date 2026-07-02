use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path as FsPath, PathBuf},
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

use crate::{
    api::{auth::current_user_id, error::ApiError, AppState},
    skills::{
        files_to_json, find_file, is_text_resource, parse_files_json, parse_skill_markdown,
        parse_skill_package, resource_storage_path, validate_resource_path, write_package_files,
        SkillFileInfo,
    },
};

const SKILL_COLUMNS: &str = "id, owner_id, name, description, body_markdown, metadata_json, \
     source, files_json, storage_path, status, created_at, updated_at";

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    body_markdown: String,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct RawImportRequest {
    raw: String,
}

#[derive(Debug, Deserialize)]
pub struct PackageImportRequest {
    #[allow(dead_code)]
    #[serde(default)]
    filename: Option<String>,
    content_base64: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubImportRequest {
    url: String,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default)]
    body_markdown: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    metadata: Option<Option<Value>>,
}

#[derive(Debug, Deserialize)]
pub struct ResourceUpdateRequest {
    content: String,
}

#[derive(Debug, Serialize)]
pub struct SkillResponse {
    id: String,
    name: String,
    description: Option<String>,
    body_markdown: String,
    metadata: Value,
    source: String,
    files: Vec<SkillFileInfo>,
    storage_path: Option<String>,
    status: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct SkillResourceResponse {
    path: String,
    size: u64,
    category: String,
    is_text: bool,
    content: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct SkillRow {
    id: String,
    owner_id: String,
    name: String,
    description: Option<String>,
    body_markdown: String,
    metadata_json: Option<String>,
    source: String,
    files_json: Option<String>,
    storage_path: Option<String>,
    status: String,
    created_at: String,
    #[allow(dead_code)]
    updated_at: String,
}

impl From<SkillRow> for SkillResponse {
    fn from(row: SkillRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            body_markdown: row.body_markdown,
            metadata: parse_json(row.metadata_json.as_deref()).unwrap_or(Value::Null),
            source: row.source,
            files: parse_files_json(row.files_json.as_deref()),
            storage_path: row.storage_path,
            status: row.status,
            created_at: row.created_at,
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let name = validate_name(&body.name)?;
    let description = normalize_description(body.description.as_deref());
    let body_markdown = validate_body_markdown(&body.body_markdown)?;
    let metadata_json = metadata_to_db_string(body.metadata.as_ref())?;
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    insert_skill(
        state.db.pool(),
        NewSkill {
            id: &id,
            owner_id: &owner_id,
            name: &name,
            description: description.as_deref(),
            body_markdown: &body_markdown,
            metadata_json: metadata_json.as_deref(),
            source: "manual",
            files_json: None,
            storage_path: None,
            now: &now,
        },
    )
    .await?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("skill vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn import_raw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RawImportRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let parsed = parse_skill_markdown(&body.raw)?;
    let metadata_json = json_to_db_string(&parsed.metadata)?;
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    insert_skill(
        state.db.pool(),
        NewSkill {
            id: &id,
            owner_id: &owner_id,
            name: &parsed.name,
            description: parsed.description.as_deref(),
            body_markdown: &parsed.body_markdown,
            metadata_json: Some(&metadata_json),
            source: "markdown",
            files_json: None,
            storage_path: None,
            now: &now,
        },
    )
    .await?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("skill vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn import_package(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PackageImportRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let bytes = STANDARD
        .decode(body.content_base64.trim())
        .map_err(|_| ApiError::invalid_input("content_base64 is not valid base64"))?;
    import_package_bytes(&state, &owner_id, &bytes, "package").await
}

pub async fn import_github(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GithubImportRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let repo = parse_github_repo(&body.url)?;
    let branch = validate_github_ref(body.branch.as_deref().unwrap_or("main"))?;
    let selected_path = match body
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        Some(path) => Some(validate_resource_path(path)?),
        None => None,
    };
    let url = format!(
        "https://codeload.github.com/{}/{}/zip/{}",
        repo.owner, repo.name, branch
    );
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|_| ApiError::invalid_input("GitHub repository archive could not be downloaded"))?
        .error_for_status()
        .map_err(|_| {
            ApiError::invalid_input("GitHub repository archive could not be downloaded")
        })?;
    let bytes = response
        .bytes()
        .await
        .map_err(|_| ApiError::invalid_input("GitHub repository archive could not be read"))?;
    const MAX_GITHUB_ZIP_BYTES: usize = 25 * 1024 * 1024;
    if bytes.len() > MAX_GITHUB_ZIP_BYTES {
        return Err(ApiError::invalid_input(
            "GitHub repository archive is too large",
        ));
    }
    let package_bytes = crop_github_skill_package(&bytes, selected_path.as_deref())?;
    import_package_bytes(&state, &owner_id, &package_bytes, "github").await
}

async fn import_package_bytes(
    state: &AppState,
    owner_id: &str,
    bytes: &[u8],
    source: &str,
) -> Result<(StatusCode, Json<SkillResponse>), ApiError> {
    let package = parse_skill_package(bytes)?;
    let metadata_json = json_to_db_string(&package.parsed.metadata)?;
    let files_json = files_to_json(&package.files)?;
    let id = Uuid::new_v4().to_string();
    let storage_dir = state.skill_storage_root.join(&id);
    write_package_files(&storage_dir, &package.payloads)?;

    let storage_path = storage_dir.to_string_lossy().into_owned();
    let now = now_rfc3339();
    insert_skill(
        state.db.pool(),
        NewSkill {
            id: &id,
            owner_id,
            name: &package.parsed.name,
            description: package.parsed.description.as_deref(),
            body_markdown: &package.parsed.body_markdown,
            metadata_json: Some(&metadata_json),
            source,
            files_json: Some(&files_json),
            storage_path: Some(&storage_path),
            now: &now,
        },
    )
    .await?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("skill vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<SkillResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let sql = format!(
        "SELECT {SKILL_COLUMNS} FROM skills \
         WHERE owner_id = ? AND status = 'active' \
         ORDER BY created_at DESC, id DESC"
    );
    let rows = sqlx::query_as::<_, SkillRow>(&sql)
        .bind(&owner_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(SkillResponse::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
) -> Result<Json<SkillResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let skill_id = validate_uuid(&skill_id, "skill id")?;
    let row = load_active_owned(state.db.pool(), &skill_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<SkillResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let skill_id = validate_uuid(&skill_id, "skill id")?;
    let existing = load_active_owned(state.db.pool(), &skill_id, &owner_id).await?;

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    let description = match body.description {
        Some(ref value) => normalize_description(value.as_deref()),
        None => existing.description.clone(),
    };
    let body_markdown = match body.body_markdown {
        Some(ref raw) => validate_body_markdown(raw)?,
        None => existing.body_markdown.clone(),
    };
    let metadata_json = match body.metadata {
        Some(ref value) => metadata_to_db_string(value.as_ref())?,
        None => existing.metadata_json.clone(),
    };

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE skills SET name = ?, description = ?, body_markdown = ?, metadata_json = ?, \
         updated_at = ? WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&description)
    .bind(&body_markdown)
    .bind(&metadata_json)
    .bind(&now)
    .bind(&skill_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update skill"))?;

    let row = fetch_row(state.db.pool(), &skill_id)
        .await?
        .ok_or_else(|| ApiError::internal("skill vanished after update"))?;
    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let skill_id = validate_uuid(&skill_id, "skill id")?;
    load_active_owned(state.db.pool(), &skill_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE skills SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&skill_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete skill"))?;

    prune_agent_skill_ids(state.db.pool(), &owner_id, &skill_id, &now).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_resources(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
) -> Result<Json<Vec<SkillResourceResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let skill_id = validate_uuid(&skill_id, "skill id")?;
    let row = load_active_owned(state.db.pool(), &skill_id, &owner_id).await?;
    let files = parse_files_json(row.files_json.as_deref());

    let resources = files
        .iter()
        .map(|file| resource_response(&state.skill_storage_root, &row, file, None))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(resources))
}

pub async fn read_resource(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((skill_id, resource_path)): Path<(String, String)>,
) -> Result<Json<SkillResourceResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let skill_id = validate_uuid(&skill_id, "skill id")?;
    let resource_path = validate_resource_path(&resource_path)?;
    let row = load_active_owned(state.db.pool(), &skill_id, &owner_id).await?;
    let files = parse_files_json(row.files_json.as_deref());
    let file = find_file(&files, &resource_path)?;

    let path = skill_resource_path(&state.skill_storage_root, &row, &resource_path)?;
    let size = file_size(&path)?;
    if !is_text_resource(&path, size) {
        return Ok(Json(SkillResourceResponse {
            path: file.path.clone(),
            size,
            category: file.category.clone(),
            is_text: false,
            content: None,
        }));
    }

    let content = fs::read_to_string(&path)
        .map_err(|_| ApiError::internal("failed to read skill resource"))?;
    Ok(Json(SkillResourceResponse {
        path: file.path.clone(),
        size,
        category: file.category.clone(),
        is_text: true,
        content: Some(content),
    }))
}

pub async fn update_resource(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((skill_id, resource_path)): Path<(String, String)>,
    Json(body): Json<ResourceUpdateRequest>,
) -> Result<Json<SkillResourceResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let skill_id = validate_uuid(&skill_id, "skill id")?;
    let resource_path = validate_resource_path(&resource_path)?;
    let row = load_active_owned(state.db.pool(), &skill_id, &owner_id).await?;
    let mut files = parse_files_json(row.files_json.as_deref());
    let file_index = files
        .iter()
        .position(|file| file.path == resource_path)
        .ok_or_else(|| ApiError::not_found("skill resource not found"))?;
    let path = skill_resource_path(&state.skill_storage_root, &row, &resource_path)?;
    let current_size = file_size(&path)?;
    if !is_text_resource(&path, current_size) {
        return Err(ApiError::invalid_input("resource is not editable as text"));
    }

    fs::write(&path, body.content.as_bytes())
        .map_err(|_| ApiError::internal("failed to update skill resource"))?;
    let new_size = file_size(&path)?;
    files[file_index].size = new_size;
    let files_json = files_to_json(&files)?;
    let now = now_rfc3339();

    sqlx::query("UPDATE skills SET files_json = ?, updated_at = ? WHERE id = ? AND owner_id = ?")
        .bind(&files_json)
        .bind(&now)
        .bind(&skill_id)
        .bind(&owner_id)
        .execute(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("failed to update skill resource metadata"))?;

    let file = &files[file_index];
    Ok(Json(SkillResourceResponse {
        path: file.path.clone(),
        size: new_size,
        category: file.category.clone(),
        is_text: true,
        content: Some(body.content),
    }))
}

struct NewSkill<'a> {
    id: &'a str,
    owner_id: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    body_markdown: &'a str,
    metadata_json: Option<&'a str>,
    source: &'a str,
    files_json: Option<&'a str>,
    storage_path: Option<&'a str>,
    now: &'a str,
}

async fn insert_skill(pool: &SqlitePool, skill: NewSkill<'_>) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO skills \
         (id, owner_id, name, description, body_markdown, metadata_json, source, files_json, \
          storage_path, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(skill.id)
    .bind(skill.owner_id)
    .bind(skill.name)
    .bind(skill.description)
    .bind(skill.body_markdown)
    .bind(skill.metadata_json)
    .bind(skill.source)
    .bind(skill.files_json)
    .bind(skill.storage_path)
    .bind(skill.now)
    .bind(skill.now)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("failed to create skill"))?;
    Ok(())
}

async fn load_active_owned(
    pool: &SqlitePool,
    skill_id: &str,
    owner_id: &str,
) -> Result<SkillRow, ApiError> {
    let row = fetch_row(pool, skill_id)
        .await?
        .ok_or_else(|| ApiError::not_found("skill not found"))?;
    if row.status == "deleted" {
        return Err(ApiError::not_found("skill not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied("skill belongs to another user"));
    }
    Ok(row)
}

async fn fetch_row(pool: &SqlitePool, skill_id: &str) -> Result<Option<SkillRow>, ApiError> {
    let sql = format!("SELECT {SKILL_COLUMNS} FROM skills WHERE id = ?");
    sqlx::query_as::<_, SkillRow>(&sql)
        .bind(skill_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn prune_agent_skill_ids(
    pool: &SqlitePool,
    owner_id: &str,
    skill_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, skill_ids_json FROM agents WHERE owner_id = ? AND status = 'active'",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    for (agent_id, raw_skill_ids) in rows {
        let skill_ids = serde_json::from_str::<Vec<String>>(&raw_skill_ids).unwrap_or_default();
        let pruned = skill_ids
            .iter()
            .filter(|id| id.as_str() != skill_id)
            .cloned()
            .collect::<Vec<_>>();
        if pruned.len() == skill_ids.len() {
            continue;
        }
        let pruned_json = serde_json::to_string(&pruned)
            .map_err(|_| ApiError::internal("failed to serialize agent skill ids"))?;
        sqlx::query("UPDATE agents SET skill_ids_json = ?, updated_at = ? WHERE id = ?")
            .bind(&pruned_json)
            .bind(now)
            .bind(&agent_id)
            .execute(pool)
            .await
            .map_err(|_| ApiError::internal("failed to prune agent skill references"))?;
    }

    Ok(())
}

fn resource_response(
    storage_root: &FsPath,
    row: &SkillRow,
    file: &SkillFileInfo,
    content: Option<String>,
) -> Result<SkillResourceResponse, ApiError> {
    let path = skill_resource_path(storage_root, row, &file.path)?;
    let size = file_size(&path)?;
    Ok(SkillResourceResponse {
        path: file.path.clone(),
        size,
        category: file.category.clone(),
        is_text: is_text_resource(&path, size),
        content,
    })
}

fn skill_resource_path(
    storage_root: &FsPath,
    row: &SkillRow,
    rel_path: &str,
) -> Result<PathBuf, ApiError> {
    let storage_path = row
        .storage_path
        .as_ref()
        .ok_or_else(|| ApiError::not_found("skill resources not found"))?;
    let skill_storage = PathBuf::from(storage_path);
    resource_storage_path(storage_root, &skill_storage, rel_path)
}

fn file_size(path: &FsPath) -> Result<u64, ApiError> {
    path.metadata()
        .map(|metadata| metadata.len())
        .map_err(|_| ApiError::not_found("skill resource not found"))
}

fn validate_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim().to_string();
    let len = name.chars().count();
    if !(1..=100).contains(&len) {
        return Err(ApiError::invalid_input(
            "name must be between 1 and 100 characters",
        ));
    }
    Ok(name)
}

fn validate_body_markdown(raw: &str) -> Result<String, ApiError> {
    let body = raw.trim().to_string();
    if body.is_empty() {
        return Err(ApiError::invalid_input("body_markdown must not be empty"));
    }
    Ok(body)
}

fn normalize_description(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|description| !description.is_empty())
        .map(str::to_string)
}

fn metadata_to_db_string(value: Option<&Value>) -> Result<Option<String>, ApiError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(_)) => json_to_db_string(value.unwrap()).map(Some),
        Some(_) => Err(ApiError::invalid_input("metadata must be an object")),
    }
}

fn json_to_db_string(value: &Value) -> Result<String, ApiError> {
    serde_json::to_string(value).map_err(|_| ApiError::internal("failed to serialize json"))
}

fn parse_json(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str::<Value>(value).ok())
}

struct GithubRepo {
    owner: String,
    name: String,
}

fn parse_github_repo(raw: &str) -> Result<GithubRepo, ApiError> {
    let trimmed = raw.trim().trim_end_matches(".git");
    let repo = if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else if !trimmed.contains("://") {
        trimmed
    } else {
        return Err(ApiError::invalid_input(
            "GitHub import requires an https://github.com repository URL or owner/repo",
        ));
    };
    let repo = repo.trim_matches('/');
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return Err(ApiError::invalid_input(
            "GitHub import requires a repository in owner/repo form",
        ));
    }
    validate_github_segment(owner, "owner")?;
    validate_github_segment(name, "repository")?;
    Ok(GithubRepo {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

fn validate_github_segment(value: &str, label: &str) -> Result<(), ApiError> {
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if valid && !value.starts_with('.') && !value.ends_with('.') && !value.contains("..") {
        Ok(())
    } else {
        Err(ApiError::invalid_input(format!(
            "GitHub {label} contains unsupported characters"
        )))
    }
}

fn validate_github_ref(raw: &str) -> Result<String, ApiError> {
    let value = raw.trim();
    let valid = !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("..")
        && !value.contains('\\')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'));
    if valid {
        Ok(value.to_string())
    } else {
        Err(ApiError::invalid_input(
            "GitHub branch or ref contains unsupported characters",
        ))
    }
}

fn crop_github_skill_package(
    bytes: &[u8],
    selected_path: Option<&str>,
) -> Result<Vec<u8>, ApiError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ApiError::invalid_input("GitHub archive is not a valid zip"))?;
    let root_prefix = github_archive_root(&mut archive)?;
    let selected_prefix = match selected_path {
        Some(path) => format!("{root_prefix}{}/", path.trim_matches('/')),
        None => detect_github_skill_prefix(&mut archive, &root_prefix)?,
    };
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        let options = SimpleFileOptions::default();
        let mut copied = 0usize;
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|_| ApiError::invalid_input("GitHub archive cannot be read"))?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(&selected_prefix) else {
                continue;
            };
            if rel.is_empty() {
                continue;
            }
            validate_resource_path(rel)?;
            writer
                .start_file(rel, options)
                .map_err(|_| ApiError::internal("failed to build skill package"))?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)
                .map_err(|_| ApiError::invalid_input("GitHub archive cannot be read"))?;
            writer
                .write_all(&data)
                .map_err(|_| ApiError::internal("failed to build skill package"))?;
            copied += 1;
        }
        writer
            .finish()
            .map_err(|_| ApiError::internal("failed to build skill package"))?;
        if copied == 0 {
            return Err(ApiError::invalid_input(
                "GitHub skill path did not contain any files",
            ));
        }
    }
    Ok(output.into_inner())
}

fn detect_github_skill_prefix(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    root_prefix: &str,
) -> Result<String, ApiError> {
    let mut selected: Option<(usize, String)> = None;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| ApiError::invalid_input("GitHub archive cannot be read"))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        let Some(rel) = name.strip_prefix(root_prefix) else {
            continue;
        };
        let rel = validate_resource_path(rel)?;
        if rel == "SKILL.md" || rel.ends_with("/SKILL.md") {
            let depth = rel.split('/').count();
            let prefix = match rel.rsplit_once('/') {
                Some((dir, _)) => format!("{root_prefix}{dir}/"),
                None => root_prefix.to_string(),
            };
            match &selected {
                None => selected = Some((depth, prefix)),
                Some((best_depth, _)) if depth < *best_depth => {
                    selected = Some((depth, prefix));
                }
                Some((best_depth, best_prefix))
                    if depth == *best_depth && best_prefix != &prefix =>
                {
                    return Err(ApiError::invalid_input(
                        "GitHub archive contains multiple equally shallow SKILL.md files",
                    ));
                }
                _ => {}
            }
        }
    }
    selected
        .map(|(_, prefix)| prefix)
        .ok_or_else(|| ApiError::invalid_input("GitHub archive does not contain SKILL.md"))
}

fn github_archive_root(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<String, ApiError> {
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| ApiError::invalid_input("GitHub archive cannot be read"))?;
        let name = file.name().replace('\\', "/");
        if let Some((root, _)) = name.split_once('/') {
            validate_resource_path(root)?;
            return Ok(format!("{root}/"));
        }
    }
    Err(ApiError::invalid_input("GitHub archive is empty"))
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

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use zip::{write::SimpleFileOptions, ZipWriter};

    use super::crop_github_skill_package;
    use crate::skills::parse_skill_package;

    fn codeload_zip(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        for (path, content) in entries {
            match content {
                Some(content) => {
                    writer
                        .start_file(*path, SimpleFileOptions::default())
                        .unwrap();
                    writer.write_all(content.as_bytes()).unwrap();
                }
                None => writer
                    .add_directory(*path, SimpleFileOptions::default())
                    .unwrap(),
            }
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn github_crop_without_selected_path_strips_root_and_directory_entries() {
        let zip = codeload_zip(&[
            ("repo-main/", None),
            (
                "repo-main/SKILL.md",
                Some("---\nname: Root Skill\ndescription: demo\n---\nUse this skill."),
            ),
            ("repo-main/references/", None),
            ("repo-main/references/readme.md", Some("reference")),
        ]);

        let cropped = crop_github_skill_package(&zip, None).unwrap();
        let parsed = parse_skill_package(&cropped).unwrap();

        assert_eq!(parsed.parsed.name, "Root Skill");
        assert!(parsed
            .files
            .iter()
            .any(|file| file.path == "references/readme.md"));
        assert!(parsed
            .files
            .iter()
            .all(|file| !file.path.starts_with("repo-main")));
    }
}
