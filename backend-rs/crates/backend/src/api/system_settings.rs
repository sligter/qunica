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
use crate::decision::DecisionConnection;
use crate::tools::{
    DeepSeekSearchConfig, MediaGenerationConfig, ShellPreference, TavilySearchConfig,
    WebSearchConfig,
};

const DEFAULT_WEB_SEARCH_PROVIDER: &str = "tavily";
const DEFAULT_TAVILY_SEARCH_URL: &str = "https://api.tavily.com/search";
const DEFAULT_TAVILY_MAX_RESULTS: i64 = 5;
const DEFAULT_TAVILY_SEARCH_DEPTH: &str = "basic";
const DEFAULT_DEEPSEEK_SEARCH_URL: &str = "https://api.deepseek.com/v1/tools/search";
const DEFAULT_DEEPSEEK_FETCH_URL: &str = "https://api.deepseek.com/v1/tools/fetch";
const DEFAULT_DEEPSEEK_MAX_RESULTS: i64 = 5;
const DEFAULT_MEDIA_BASE_URL: &str = "https://api.openai.com";
const DEFAULT_IMAGE_GENERATION_ENDPOINT: &str = "/v1/images/generations";
const DEFAULT_VIDEO_GENERATION_ENDPOINT: &str = "/v1/videos";
const DEFAULT_VIDEO_STATUS_ENDPOINT: &str = "/v1/videos/{id}";
const DEFAULT_VIDEO_CONTENT_ENDPOINT: &str = "/v1/videos/{id}/content";
const DEFAULT_APPEARANCE: &str = "system";
const DEFAULT_LANGUAGE: &str = "en-US";
const DEFAULT_REPLY_INSERT_MODE: &str = "instant";

const SETTINGS_COLUMNS: &str =
    "id, owner_id, appearance, language, reply_insert_mode, assistant_enabled, assistant_auto_approve, onboarding_completed, group_workspace_root, shell_preference, web_search_provider, \
     tavily_api_key, tavily_search_url, tavily_max_results, tavily_search_depth, \
     tavily_include_answer, tavily_include_raw_content, \
     deepseek_api_key, deepseek_search_url, deepseek_fetch_url, deepseek_max_results, deepseek_need_content, \
     media_base_url, media_api_key, \
     image_generation_model, image_generation_endpoint, video_generation_model, \
     video_generation_endpoint, video_status_endpoint, video_content_endpoint, \
     decision_endpoint, decision_api_key, decision_model, decision_min_confidence, \
     created_at, updated_at";

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default, deserialize_with = "double_option")]
    appearance: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    language: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    reply_insert_mode: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    assistant_enabled: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    assistant_auto_approve: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    onboarding_completed: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    group_workspace_root: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    shell_preference: Option<Option<String>>,
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
    #[serde(default, deserialize_with = "double_option")]
    deepseek_api_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    deepseek_search_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    deepseek_fetch_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    deepseek_max_results: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    deepseek_need_content: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    media_base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    media_api_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    image_generation_model: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    image_generation_endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    video_generation_model: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    video_generation_endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    video_status_endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    video_content_endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    decision_endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    decision_api_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    decision_model: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    decision_min_confidence: Option<Option<f64>>,
}

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    id: String,
    owner_id: String,
    appearance: String,
    language: String,
    reply_insert_mode: String,
    assistant_enabled: bool,
    assistant_auto_approve: bool,
    onboarding_completed: bool,
    group_workspace_root: Option<String>,
    shell_preference: String,
    web_search_provider: String,
    tavily_api_key_configured: bool,
    tavily_search_url: String,
    tavily_max_results: i64,
    tavily_search_depth: String,
    tavily_include_answer: bool,
    tavily_include_raw_content: bool,
    deepseek_api_key_configured: bool,
    deepseek_search_url: String,
    deepseek_fetch_url: String,
    deepseek_max_results: i64,
    deepseek_need_content: bool,
    media_base_url: String,
    media_api_key_configured: bool,
    image_generation_model: Option<String>,
    image_generation_endpoint: String,
    video_generation_model: Option<String>,
    video_generation_endpoint: String,
    video_status_endpoint: String,
    video_content_endpoint: String,
    decision_endpoint: String,
    decision_api_key_configured: bool,
    decision_model: String,
    decision_min_confidence: f64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct SettingsRow {
    id: String,
    owner_id: String,
    appearance: String,
    language: String,
    reply_insert_mode: String,
    assistant_enabled: i64,
    assistant_auto_approve: i64,
    onboarding_completed: i64,
    group_workspace_root: Option<String>,
    shell_preference: String,
    web_search_provider: String,
    tavily_api_key: Option<String>,
    tavily_search_url: String,
    tavily_max_results: i64,
    tavily_search_depth: String,
    tavily_include_answer: i64,
    tavily_include_raw_content: i64,
    deepseek_api_key: Option<String>,
    deepseek_search_url: String,
    deepseek_fetch_url: String,
    deepseek_max_results: i64,
    deepseek_need_content: i64,
    media_base_url: String,
    media_api_key: Option<String>,
    image_generation_model: Option<String>,
    image_generation_endpoint: String,
    video_generation_model: Option<String>,
    video_generation_endpoint: String,
    video_status_endpoint: String,
    video_content_endpoint: String,
    decision_endpoint: String,
    decision_api_key: Option<String>,
    decision_model: String,
    decision_min_confidence: f64,
    created_at: String,
    updated_at: String,
}

impl SettingsRow {
    fn decision_connection(&self) -> DecisionConnection {
        DecisionConnection {
            endpoint: self.decision_endpoint.clone(),
            api_key: self.decision_api_key.clone(),
            model: self.decision_model.clone(),
            min_confidence: self.decision_min_confidence,
        }
    }
}

impl From<SettingsRow> for SettingsResponse {
    fn from(row: SettingsRow) -> Self {
        let decision = row.decision_connection();
        Self {
            id: row.id,
            owner_id: row.owner_id,
            appearance: row.appearance,
            language: row.language,
            reply_insert_mode: row.reply_insert_mode,
            assistant_enabled: row.assistant_enabled != 0,
            assistant_auto_approve: row.assistant_auto_approve != 0,
            onboarding_completed: row.onboarding_completed != 0,
            group_workspace_root: row.group_workspace_root,
            shell_preference: row.shell_preference,
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
            deepseek_api_key_configured: row
                .deepseek_api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty()),
            deepseek_search_url: row.deepseek_search_url,
            deepseek_fetch_url: row.deepseek_fetch_url,
            deepseek_max_results: row.deepseek_max_results,
            deepseek_need_content: row.deepseek_need_content != 0,
            media_base_url: row.media_base_url,
            media_api_key_configured: row
                .media_api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty()),
            image_generation_model: row.image_generation_model,
            image_generation_endpoint: row.image_generation_endpoint,
            video_generation_model: row.video_generation_model,
            video_generation_endpoint: row.video_generation_endpoint,
            video_status_endpoint: row.video_status_endpoint,
            video_content_endpoint: row.video_content_endpoint,
            decision_api_key_configured: decision.is_configured(),
            decision_endpoint: decision.endpoint,
            decision_model: decision.model,
            decision_min_confidence: decision.min_confidence,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// The owner's decision-endpoint connection, for the runtime gate.
pub(crate) async fn decision_connection(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<DecisionConnection, ApiError> {
    let row = get_or_create(pool, owner_id).await?;
    Ok(row.decision_connection())
}

pub(crate) async fn web_search_config(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<Option<WebSearchConfig>, ApiError> {
    let row = get_or_create(pool, owner_id).await?;
    // Note: keys belong to each search provider, independently of chat model
    // providers; switching never reuses another provider's saved credentials.
    match row.web_search_provider.as_str() {
        "tavily" => Ok(normalize_key(row.tavily_api_key.as_deref()).map(|api_key| {
            WebSearchConfig::Tavily(TavilySearchConfig {
                api_key,
                search_url: row.tavily_search_url,
                max_results: row.tavily_max_results.clamp(1, 20) as u32,
                search_depth: row.tavily_search_depth,
                include_answer: row.tavily_include_answer != 0,
                include_raw_content: row.tavily_include_raw_content != 0,
            })
        })),
        "deepseek" => Ok(
            normalize_key(row.deepseek_api_key.as_deref()).map(|api_key| {
                WebSearchConfig::DeepSeek(DeepSeekSearchConfig {
                    api_key,
                    search_url: row.deepseek_search_url,
                    fetch_url: row.deepseek_fetch_url,
                    max_results: row.deepseek_max_results.clamp(1, 50) as u32,
                    need_content: row.deepseek_need_content != 0,
                })
            }),
        ),
        _ => Ok(None),
    }
}

/// The interpreter this account wants its shells started with.
///
/// A row that predates the setting, or holds a value this build no longer
/// recognises, reads as [`ShellPreference::Auto`]: the shell tool keeps working
/// under the host's own probe order rather than failing over a settings value.
pub(crate) async fn shell_preference(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<ShellPreference, ApiError> {
    let row = get_or_create(pool, owner_id).await?;
    Ok(ShellPreference::parse(&row.shell_preference).unwrap_or_default())
}

pub(crate) async fn media_generation_config(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<Option<MediaGenerationConfig>, ApiError> {
    let row = get_or_create(pool, owner_id).await?;
    let Some(api_key) = row
        .media_api_key
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(MediaGenerationConfig {
        api_key,
        base_url: row.media_base_url,
        image_model: normalized_optional_text(row.image_generation_model),
        image_endpoint: row.image_generation_endpoint,
        video_model: normalized_optional_text(row.video_generation_model),
        video_endpoint: row.video_generation_endpoint,
        video_status_endpoint: row.video_status_endpoint,
        video_content_endpoint: row.video_content_endpoint,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let mut row = get_or_create(state.db.pool(), &owner_id).await?;
    if row.onboarding_completed == 0 && row.group_workspace_root.is_none() {
        if let Some(root) = state.default_group_workspace_root.as_deref() {
            let now = now_rfc3339();
            sqlx::query(
                "UPDATE system_settings SET group_workspace_root = ?, updated_at = ? \
                 WHERE owner_id = ? AND onboarding_completed = 0 AND group_workspace_root IS NULL",
            )
            .bind(root.to_string_lossy().as_ref())
            .bind(&now)
            .bind(&owner_id)
            .execute(state.db.pool())
            .await
            .map_err(|_| ApiError::internal("failed to apply default workspace root"))?;
            row = fetch_by_owner(state.db.pool(), &owner_id)
                .await?
                .ok_or_else(|| ApiError::internal("system settings vanished after update"))?;
        }
    }
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<SettingsResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let existing = get_or_create(state.db.pool(), &owner_id).await?;

    let appearance = match body.appearance {
        Some(ref value) => normalize_appearance(value.as_deref())?,
        None => existing.appearance.clone(),
    };
    let language = match body.language {
        Some(ref value) => normalize_language(value.as_deref())?,
        None => existing.language.clone(),
    };
    let reply_insert_mode = match body.reply_insert_mode {
        Some(ref value) => normalize_reply_insert_mode(value.as_deref())?,
        None => existing.reply_insert_mode.clone(),
    };
    let assistant_enabled = match body.assistant_enabled {
        Some(Some(value)) => value,
        Some(None) => true,
        None => existing.assistant_enabled != 0,
    };
    let assistant_auto_approve = match body.assistant_auto_approve {
        Some(Some(value)) => value,
        Some(None) => false,
        None => existing.assistant_auto_approve != 0,
    };
    let onboarding_completed = match body.onboarding_completed {
        Some(Some(value)) => value,
        Some(None) => false,
        None => existing.onboarding_completed != 0,
    };
    let group_workspace_root = match body.group_workspace_root {
        Some(ref value) => normalize_root(value.as_deref())?,
        None => existing.group_workspace_root.clone(),
    };
    let shell_preference = match body.shell_preference {
        Some(ref value) => normalize_shell_preference(value.as_deref())?,
        None => existing.shell_preference.clone(),
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
    let deepseek_api_key = match body.deepseek_api_key {
        Some(ref value) => normalize_key(value.as_deref()),
        None => existing.deepseek_api_key.clone(),
    };
    let deepseek_search_url = match body.deepseek_search_url {
        Some(ref value) => normalize_http_url(
            value.as_deref(),
            DEFAULT_DEEPSEEK_SEARCH_URL,
            "deepseek_search_url",
        )?,
        None => existing.deepseek_search_url.clone(),
    };
    let deepseek_fetch_url = match body.deepseek_fetch_url {
        Some(ref value) => normalize_http_url(
            value.as_deref(),
            DEFAULT_DEEPSEEK_FETCH_URL,
            "deepseek_fetch_url",
        )?,
        None => existing.deepseek_fetch_url.clone(),
    };
    let deepseek_max_results = match body.deepseek_max_results {
        Some(value) => {
            let value = value.unwrap_or(DEFAULT_DEEPSEEK_MAX_RESULTS);
            if !(1..=50).contains(&value) {
                return Err(ApiError::invalid_input(
                    "deepseek_max_results must be between 1 and 50",
                ));
            }
            value
        }
        None => existing.deepseek_max_results,
    };
    let deepseek_need_content = match body.deepseek_need_content {
        Some(value) => value.unwrap_or(false),
        None => existing.deepseek_need_content != 0,
    };
    let media_base_url = match body.media_base_url {
        Some(ref value) => normalize_media_base_url(value.as_deref())?,
        None => existing.media_base_url.clone(),
    };
    let media_api_key = match body.media_api_key {
        Some(ref value) => normalize_key(value.as_deref()),
        None => existing.media_api_key.clone(),
    };
    let image_generation_model = match body.image_generation_model {
        Some(value) => normalize_model(value, "image_generation_model")?,
        None => existing.image_generation_model.clone(),
    };
    let image_generation_endpoint = match body.image_generation_endpoint {
        Some(ref value) => normalize_media_endpoint(
            value.as_deref(),
            DEFAULT_IMAGE_GENERATION_ENDPOINT,
            "image_generation_endpoint",
            false,
        )?,
        None => existing.image_generation_endpoint.clone(),
    };
    let video_generation_model = match body.video_generation_model {
        Some(value) => normalize_model(value, "video_generation_model")?,
        None => existing.video_generation_model.clone(),
    };
    let video_generation_endpoint = match body.video_generation_endpoint {
        Some(ref value) => normalize_media_endpoint(
            value.as_deref(),
            DEFAULT_VIDEO_GENERATION_ENDPOINT,
            "video_generation_endpoint",
            false,
        )?,
        None => existing.video_generation_endpoint.clone(),
    };
    let video_status_endpoint = match body.video_status_endpoint {
        Some(ref value) => normalize_media_endpoint(
            value.as_deref(),
            DEFAULT_VIDEO_STATUS_ENDPOINT,
            "video_status_endpoint",
            true,
        )?,
        None => existing.video_status_endpoint.clone(),
    };
    let video_content_endpoint = match body.video_content_endpoint {
        Some(ref value) => normalize_media_endpoint(
            value.as_deref(),
            DEFAULT_VIDEO_CONTENT_ENDPOINT,
            "video_content_endpoint",
            true,
        )?,
        None => existing.video_content_endpoint.clone(),
    };
    let decision_endpoint = match body.decision_endpoint {
        Some(ref value) => normalize_decision_endpoint(value.as_deref())?,
        None => existing.decision_endpoint.clone(),
    };
    let decision_api_key = match body.decision_api_key {
        Some(ref value) => normalize_key(value.as_deref()),
        None => existing.decision_api_key.clone(),
    };
    let decision_model = match body.decision_model {
        Some(value) => normalize_model(value, "decision_model")?
            .unwrap_or_else(|| crate::decision::DEFAULT_MODEL.to_string()),
        None => existing.decision_model.clone(),
    };
    let decision_min_confidence = match body.decision_min_confidence {
        Some(value) => normalize_min_confidence(value)?,
        None => existing.decision_min_confidence,
    };

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE system_settings SET \
         appearance = ?, language = ?, reply_insert_mode = ?, assistant_enabled = ?, assistant_auto_approve = ?, onboarding_completed = ?, group_workspace_root = ?, shell_preference = ?, web_search_provider = ?, tavily_api_key = ?, \
         tavily_search_url = ?, tavily_max_results = ?, tavily_search_depth = ?, \
         tavily_include_answer = ?, tavily_include_raw_content = ?, \
         deepseek_api_key = ?, deepseek_search_url = ?, deepseek_fetch_url = ?, deepseek_max_results = ?, deepseek_need_content = ?, \
         media_base_url = ?, media_api_key = ?, \
         image_generation_model = ?, image_generation_endpoint = ?, video_generation_model = ?, \
         video_generation_endpoint = ?, video_status_endpoint = ?, video_content_endpoint = ?, \
         decision_endpoint = ?, decision_api_key = ?, decision_model = ?, \
         decision_min_confidence = ?, updated_at = ? \
         WHERE owner_id = ?",
    )
    .bind(&appearance)
    .bind(&language)
    .bind(&reply_insert_mode)
    .bind(if assistant_enabled { 1_i64 } else { 0_i64 })
    .bind(if assistant_auto_approve { 1_i64 } else { 0_i64 })
    .bind(if onboarding_completed { 1_i64 } else { 0_i64 })
    .bind(&group_workspace_root)
    .bind(&shell_preference)
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
    .bind(&deepseek_api_key)
    .bind(&deepseek_search_url)
    .bind(&deepseek_fetch_url)
    .bind(deepseek_max_results)
    .bind(if deepseek_need_content { 1_i64 } else { 0_i64 })
    .bind(&media_base_url)
    .bind(&media_api_key)
    .bind(&image_generation_model)
    .bind(&image_generation_endpoint)
    .bind(&video_generation_model)
    .bind(&video_generation_endpoint)
    .bind(&video_status_endpoint)
    .bind(&video_content_endpoint)
    .bind(&decision_endpoint)
    .bind(&decision_api_key)
    .bind(&decision_model)
    .bind(decision_min_confidence)
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

/// What the connectivity test sends: an optional key to try before saving it.
/// Every other field falls back to the saved settings.
#[derive(Debug, Default, Deserialize)]
pub struct DecisionTestRequest {
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DecisionTestResponse {
    ok: bool,
    /// The versioned model the endpoint reported, when it did. The gateway
    /// dialect reports none.
    model: Option<String>,
    /// The probability the sample sentence conveys urgency; a sanity check
    /// that the endpoint is a decision endpoint and not a chat one.
    sample_probability: Option<f64>,
    input_tokens: Option<i64>,
    message: String,
    /// Which wire dialect the endpoint URL selected.
    dialect: &'static str,
}

/// Send one small request to the decision endpoint and report whether it
/// answered. Uses the saved key unless the body carries one.
pub async fn test_decision(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<DecisionTestRequest>>,
) -> Result<Json<DecisionTestResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state).await?;
    let body = body.map(|Json(body)| body).unwrap_or_default();
    let saved = get_or_create(state.db.pool(), &owner_id).await?;
    let endpoint = match body.endpoint {
        Some(ref value) => normalize_decision_endpoint(Some(value))?,
        None => saved.decision_endpoint.clone(),
    };
    let model = normalize_model(body.model, "model")?.unwrap_or(saved.decision_model.clone());
    let api_key = normalize_key(body.api_key.as_deref())
        .or_else(|| normalize_key(saved.decision_api_key.as_deref()))
        .ok_or_else(|| ApiError::invalid_input("a decision API key is required"))?;

    let client = crate::decision::DecisionClient::new(
        &endpoint,
        api_key,
        &model,
        crate::decision::DECISION_TIMEOUT,
    )
    .map_err(|error| ApiError::invalid_input(error.to_string()))?;
    let questions = std::collections::BTreeMap::from([(
        "is_urgent".to_string(),
        crate::decision::Question::noul(
            "Does this message convey urgency?",
            "Explicitly time-sensitive",
            "No urgency expressed",
        ),
    )]);
    match client
        .evaluate(
            serde_json::Value::String("Help! My payouts have been failing for 3 days.".to_string()),
            questions,
        )
        .await
    {
        Ok(response) => {
            let usage = response.usage.clone().unwrap_or_default();
            let sample_probability = response.noul("is_urgent");
            let message = if sample_probability.is_some() {
                "The decision endpoint answered.".to_string()
            } else {
                "The endpoint answered, but not with a decision: check that it is a System One \
                 endpoint and the model is a decision model."
                    .to_string()
            };
            Ok(Json(DecisionTestResponse {
                ok: sample_probability.is_some(),
                model: response.model,
                sample_probability,
                input_tokens: usage.input_tokens,
                message,
                dialect: client.dialect().as_str(),
            }))
        }
        Err(error) => {
            if let crate::decision::DecisionError::Http { status, body } = &error {
                tracing::warn!(status, body, "decision endpoint test failed");
            }
            Ok(Json(DecisionTestResponse {
                ok: false,
                model: None,
                sample_probability: None,
                input_tokens: None,
                message: error.safe_message(),
                dialect: client.dialect().as_str(),
            }))
        }
    }
}

async fn get_or_create(pool: &SqlitePool, owner_id: &str) -> Result<SettingsRow, ApiError> {
    if let Some(row) = fetch_by_owner(pool, owner_id).await? {
        return Ok(row);
    }

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO system_settings \
         (id, owner_id, appearance, language, assistant_auto_approve, group_workspace_root, web_search_provider, tavily_api_key, \
          tavily_search_url, tavily_max_results, tavily_search_depth, \
          tavily_include_answer, tavily_include_raw_content, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, NULL, ?, NULL, ?, ?, ?, 1, 0, ?, ?)",
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

/// Accept the interpreter names the settings UI offers, plus the aliases a
/// person might type for the same shell, and store the canonical value.
fn normalize_shell_preference(raw: Option<&str>) -> Result<String, ApiError> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(ShellPreference::default().as_str().to_string());
    };
    ShellPreference::parse(value)
        .map(|preference| preference.as_str().to_string())
        .ok_or_else(|| {
            ApiError::invalid_input(
                "shell_preference must be 'auto', 'powershell', 'bash', or 'cmd'",
            )
        })
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

/// What the composer does with a message typed while a reply is still running.
///
/// Unknown values are refused rather than coerced: the two modes send the
/// message to different turns, so silently picking one would answer a question
/// the user did not ask.
fn normalize_reply_insert_mode(raw: Option<&str>) -> Result<String, ApiError> {
    let mode = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_REPLY_INSERT_MODE)
        .to_lowercase();
    match mode.as_str() {
        "instant" | "queue" => Ok(mode),
        _ => Err(ApiError::invalid_input(
            "reply_insert_mode must be 'instant' or 'queue'",
        )),
    }
}

fn normalize_provider(raw: Option<&str>) -> Result<String, ApiError> {
    let provider = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_WEB_SEARCH_PROVIDER)
        .to_lowercase();
    if matches!(provider.as_str(), "tavily" | "deepseek") {
        Ok(provider)
    } else {
        Err(ApiError::invalid_input(
            "web_search_provider must be 'tavily' or 'deepseek'",
        ))
    }
}

fn normalize_key(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_model(raw: Option<String>, field: &str) -> Result<Option<String>, ApiError> {
    let model = raw
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if model
        .as_deref()
        .is_some_and(|value| value.chars().count() > 200)
    {
        return Err(ApiError::invalid_input(format!(
            "{field} must be at most 200 characters"
        )));
    }
    Ok(model)
}

fn normalized_optional_text(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_tavily_url(raw: Option<&str>) -> Result<String, ApiError> {
    normalize_http_url(raw, DEFAULT_TAVILY_SEARCH_URL, "tavily_search_url")
}

fn normalize_media_base_url(raw: Option<&str>) -> Result<String, ApiError> {
    normalize_http_url(raw, DEFAULT_MEDIA_BASE_URL, "media_base_url")
}

/// The full decision URL, not a base: TypeSafe serves `/v1/systemone` and
/// OpenRouter `/api/alpha/decisions`, so there is no path to append.
fn normalize_decision_endpoint(raw: Option<&str>) -> Result<String, ApiError> {
    normalize_http_url(raw, crate::decision::DEFAULT_ENDPOINT, "decision_endpoint")
}

fn normalize_min_confidence(raw: Option<f64>) -> Result<f64, ApiError> {
    let value = raw.unwrap_or(crate::decision::DEFAULT_MIN_CONFIDENCE);
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::invalid_input(
            "decision_min_confidence must be between 0 and 1",
        ))
    }
}

fn normalize_http_url(raw: Option<&str>, default: &str, field: &str) -> Result<String, ApiError> {
    let url = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .trim_end_matches('/');
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ApiError::invalid_input(format!("{field} must be an http or https URL")))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(url.to_string()),
        _ => Err(ApiError::invalid_input(format!(
            "{field} must be an http or https URL"
        ))),
    }
}

fn normalize_media_endpoint(
    raw: Option<&str>,
    default: &str,
    field: &str,
    requires_id: bool,
) -> Result<String, ApiError> {
    let endpoint = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default);
    if endpoint.chars().count() > 2_048 {
        return Err(ApiError::invalid_input(format!(
            "{field} must be at most 2048 characters"
        )));
    }
    if requires_id && !endpoint.contains("{id}") {
        return Err(ApiError::invalid_input(format!(
            "{field} must contain {{id}}"
        )));
    }
    if endpoint.starts_with('/') {
        return Ok(endpoint.to_string());
    }
    let parsed = reqwest::Url::parse(&endpoint.replace("{id}", "example"))
        .map_err(|_| ApiError::invalid_input(format!("{field} must be an endpoint path or URL")))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(endpoint.to_string()),
        _ => Err(ApiError::invalid_input(format!(
            "{field} must be an endpoint path or URL"
        ))),
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
