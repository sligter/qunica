use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::tools::TavilySearchConfig;

const DEFAULT_WEB_SEARCH_PROVIDER: &str = "tavily";
const DEFAULT_TAVILY_SEARCH_URL: &str = "https://api.tavily.com/search";
const DEFAULT_TAVILY_MAX_RESULTS: i64 = 5;
const DEFAULT_TAVILY_SEARCH_DEPTH: &str = "basic";
const DEFAULT_APPEARANCE: &str = "system";
const DEFAULT_LANGUAGE: &str = "en-US";

const SETTINGS_COLUMNS: &str =
    "id, owner_id, appearance, language, group_workspace_root, web_search_provider, \
     tavily_api_key, tavily_search_url, tavily_max_results, tavily_search_depth, \
     tavily_include_answer, tavily_include_raw_content, created_at, updated_at";

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default, deserialize_with = "double_option")]
    appearance: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    language: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    group_workspace_root: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    web_search_provider: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    tavily_api_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    tavily_search_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    tavily_max_results: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    tavily_search_depth: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    tavily_include_answer: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    tavily_include_raw_content: Option<Option<bool>>,
}

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    id: String,
    owner_id: String,
    appearance: String,
    language: String,
    group_workspace_root: Option<String>,
    web_search_provider: String,
    tavily_api_key_configured: bool,
    tavily_search_url: String,
    tavily_max_results: i64,
    tavily_search_depth: String,
    tavily_include_answer: bool,
    tavily_include_raw_content: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct SettingsRow {
    id: String,
    owner_id: String,
    appearance: String,
    language: String,
    group_workspace_root: Option<String>,
    web_search_provider: String,
    tavily_api_key: Option<String>,
    tavily_search_url: String,
    tavily_max_results: i64,
    tavily_search_depth: String,
    tavily_include_answer: i64,
    tavily_include_raw_content: i64,
    created_at: String,
    updated_at: String,
}

impl From<SettingsRow> for SettingsResponse {
    fn from(row: SettingsRow) -> Self {
        Self {
            id: row.id,
            owner_id: row.owner_id,
            appearance: row.appearance,
            language: row.language,
            group_workspace_root: row.group_workspace_root,
            web_search_provider: row.web_search_provider,
            tavily_api_key_configured: row
                .tavily_api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty()),
            tavily_search_url: row.tavily_search_url,
            tavily_max_results: row.tavily_max_results,
            tavily_search_depth: row.tavily_search_depth,
            tavily_include_answer: row.tavily_include_answer != 0,
            tavily_include_raw_content: row.tavily_include_raw_content != 0,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub(crate) async fn tavily_search_config(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<Option<TavilySearchConfig>, ApiError> {
    let row = get_or_create(pool, owner_id).await?;
    let Some(api_key) = row
        .tavily_api_key
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
    else {
        return Ok(None);
    };
    if row.web_search_provider != DEFAULT_WEB_SEARCH_PROVIDER {
        return Ok(None);
    }
    Ok(Some(TavilySearchConfig {
        api_key,
        search_url: row.tavily_search_url,
        max_results: row.tavily_max_results.clamp(1, 20) as u32,
        search_depth: row.tavily_search_depth,
        include_answer: row.tavily_include_answer != 0,
        include_raw_content: row.tavily_include_raw_content != 0,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let row = get_or_create(state.db.pool(), &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<SettingsResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let existing = get_or_create(state.db.pool(), &owner_id).await?;

    let appearance = match body.appearance {
        Some(ref value) => normalize_appearance(value.as_deref())?,
        None => existing.appearance.clone(),
    };
    let language = match body.language {
        Some(ref value) => normalize_language(value.as_deref())?,
        None => existing.language.clone(),
    };
    let group_workspace_root = match body.group_workspace_root {
        Some(ref value) => normalize_root(value.as_deref())?,
        None => existing.group_workspace_root.clone(),
    };
    let web_search_provider = match body.web_search_provider {
        Some(ref value) => normalize_provider(value.as_deref())?,
        None => existing.web_search_provider.clone(),
    };
    let tavily_api_key = match body.tavily_api_key {
        Some(ref value) => normalize_key(value.as_deref()),
        None => existing.tavily_api_key.clone(),
    };
    let tavily_search_url = match body.tavily_search_url {
        Some(ref value) => normalize_tavily_url(value.as_deref())?,
        None => existing.tavily_search_url.clone(),
    };
    let tavily_max_results = match body.tavily_max_results {
        Some(value) => normalize_max_results(value)?,
        None => existing.tavily_max_results,
    };
    let tavily_search_depth = match body.tavily_search_depth {
        Some(ref value) => normalize_search_depth(value.as_deref())?,
        None => existing.tavily_search_depth.clone(),
    };
    let tavily_include_answer = match body.tavily_include_answer {
        Some(Some(value)) => value,
        Some(None) => true,
        None => existing.tavily_include_answer != 0,
    };
    let tavily_include_raw_content = match body.tavily_include_raw_content {
        Some(Some(value)) => value,
        Some(None) => false,
        None => existing.tavily_include_raw_content != 0,
    };

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE system_settings SET \
         appearance = ?, language = ?, group_workspace_root = ?, web_search_provider = ?, tavily_api_key = ?, \
         tavily_search_url = ?, tavily_max_results = ?, tavily_search_depth = ?, \
         tavily_include_answer = ?, tavily_include_raw_content = ?, updated_at = ? \
         WHERE owner_id = ?",
    )
    .bind(&appearance)
    .bind(&language)
    .bind(&group_workspace_root)
    .bind(&web_search_provider)
    .bind(&tavily_api_key)
    .bind(&tavily_search_url)
    .bind(tavily_max_results)
    .bind(&tavily_search_depth)
    .bind(if tavily_include_answer { 1_i64 } else { 0_i64 })
    .bind(if tavily_include_raw_content {
        1_i64
    } else {
        0_i64
    })
    .bind(&now)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update system settings"))?;

    let row = fetch_by_owner(state.db.pool(), &owner_id)
        .await?
        .ok_or_else(|| ApiError::internal("system settings vanished after update"))?;
    Ok(Json(row.into()))
}

async fn get_or_create(pool: &SqlitePool, owner_id: &str) -> Result<SettingsRow, ApiError> {
    if let Some(row) = fetch_by_owner(pool, owner_id).await? {
        return Ok(row);
    }

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO system_settings \
         (id, owner_id, appearance, language, group_workspace_root, web_search_provider, tavily_api_key, \
          tavily_search_url, tavily_max_results, tavily_search_depth, \
          tavily_include_answer, tavily_include_raw_content, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NULL, ?, NULL, ?, ?, ?, 1, 0, ?, ?)",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(DEFAULT_APPEARANCE)
    .bind(DEFAULT_LANGUAGE)
    .bind(DEFAULT_WEB_SEARCH_PROVIDER)
    .bind(DEFAULT_TAVILY_SEARCH_URL)
    .bind(DEFAULT_TAVILY_MAX_RESULTS)
    .bind(DEFAULT_TAVILY_SEARCH_DEPTH)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("failed to create system settings"))?;

    fetch_by_owner(pool, owner_id)
        .await?
        .ok_or_else(|| ApiError::internal("system settings vanished after insert"))
}

async fn fetch_by_owner(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<Option<SettingsRow>, ApiError> {
    let sql = format!("SELECT {SETTINGS_COLUMNS} FROM system_settings WHERE owner_id = ?");
    sqlx::query_as::<_, SettingsRow>(&sql)
        .bind(owner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

fn normalize_root(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(path) = raw.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        ApiError::invalid_input("group_workspace_root must be an existing directory")
    })?;
    if !canonical.is_dir() {
        return Err(ApiError::invalid_input(
            "group_workspace_root must be an existing directory",
        ));
    }
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

fn normalize_appearance(raw: Option<&str>) -> Result<String, ApiError> {
    let appearance = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_APPEARANCE)
        .to_lowercase();
    match appearance.as_str() {
        "light" | "dark" | "system" => Ok(appearance),
        _ => Err(ApiError::invalid_input(
            "appearance must be 'light', 'dark', or 'system'",
        )),
    }
}

fn normalize_language(raw: Option<&str>) -> Result<String, ApiError> {
    let language = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_LANGUAGE);
    match language {
        "zh-CN" | "en-US" => Ok(language.to_string()),
        _ => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_input",
            "language must be 'zh-CN' or 'en-US'",
        )),
    }
}

fn normalize_provider(raw: Option<&str>) -> Result<String, ApiError> {
    let provider = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_WEB_SEARCH_PROVIDER)
        .to_lowercase();
    if provider == DEFAULT_WEB_SEARCH_PROVIDER {
        Ok(provider)
    } else {
        Err(ApiError::invalid_input(
            "web_search_provider must be 'tavily'",
        ))
    }
}

fn normalize_key(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_tavily_url(raw: Option<&str>) -> Result<String, ApiError> {
    let Some(url) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(DEFAULT_TAVILY_SEARCH_URL.to_string());
    };
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ApiError::invalid_input("tavily_search_url must be an http or https URL"))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(url.to_string()),
        _ => Err(ApiError::invalid_input(
            "tavily_search_url must be an http or https URL",
        )),
    }
}

fn normalize_max_results(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(DEFAULT_TAVILY_MAX_RESULTS);
    if (1..=20).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::invalid_input(
            "tavily_max_results must be between 1 and 20",
        ))
    }
}

fn normalize_search_depth(raw: Option<&str>) -> Result<String, ApiError> {
    let depth = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TAVILY_SEARCH_DEPTH)
        .to_lowercase();
    match depth.as_str() {
        "basic" | "advanced" => Ok(depth),
        _ => Err(ApiError::invalid_input(
            "tavily_search_depth must be 'basic' or 'advanced'",
        )),
    }
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
