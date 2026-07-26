use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::llm::{
    discover_models, ModelCatalogError, ModelInfo, ProviderConfig, ProviderModelConfig,
    MODEL_CATALOG_TIMEOUT,
};

const PROVIDER_COLUMNS: &str = "id, owner_id, name, kind, base_url, api_key, default_model, \
     context_window_tokens, context_output_reserve_ratio, description, reasoning_passback, \
     models_json, status, created_at, updated_at";

const VALID_KINDS: [&str; 4] = [
    "openai-compatible",
    "anthropic",
    "anthropic-compatible",
    "gemini",
];

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    kind: String,
    #[serde(default)]
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    #[serde(default)]
    context_window_tokens: Option<i64>,
    #[serde(default)]
    context_output_reserve_ratio: Option<f64>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    reasoning_passback: Option<bool>,
    #[serde(default)]
    models: Option<Vec<ProviderModelConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    api_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    default_model: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    context_window_tokens: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    context_output_reserve_ratio: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default)]
    reasoning_passback: Option<bool>,
    #[serde(default)]
    models: Option<Vec<ProviderModelConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoverRequest {
    kind: String,
    #[serde(default)]
    base_url: Option<String>,
    api_key: String,
    #[serde(default)]
    default_model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    id: String,
    name: String,
    kind: String,
    base_url: Option<String>,
    api_key_masked: String,
    default_model: String,
    context_window_tokens: Option<i64>,
    context_output_reserve_ratio: Option<f64>,
    description: Option<String>,
    reasoning_passback: bool,
    models: Vec<ProviderModelConfig>,
    status: String,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ProviderRow {
    id: String,
    owner_id: String,
    name: String,
    kind: String,
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    context_window_tokens: Option<i64>,
    context_output_reserve_ratio: Option<f64>,
    description: Option<String>,
    reasoning_passback: i64,
    models_json: Option<String>,
    status: String,
    created_at: String,
    #[allow(dead_code)]
    updated_at: String,
}

impl From<ProviderRow> for ProviderResponse {
    fn from(row: ProviderRow) -> Self {
        let models = stored_models(&row);
        Self {
            id: row.id,
            name: row.name,
            kind: row.kind,
            base_url: row.base_url,
            api_key_masked: mask_api_key(&row.api_key),
            default_model: row.default_model,
            context_window_tokens: row.context_window_tokens,
            context_output_reserve_ratio: row.context_output_reserve_ratio,
            description: row.description,
            reasoning_passback: row.reasoning_passback != 0,
            models,
            status: row.status,
            created_at: row.created_at,
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let name = validate_name(&body.name)?;
    let kind = validate_kind(&body.kind)?;
    let base_url = normalize_nullable_text(body.base_url.as_deref());
    let api_key = validate_api_key(&body.api_key)?;
    let default_model = validate_default_model(&body.default_model)?;
    validate_context_window(body.context_window_tokens)?;
    validate_reserve_ratio(body.context_output_reserve_ratio)?;
    let description = normalize_nullable_text(body.description.as_deref());
    let reasoning_passback = body.reasoning_passback.unwrap_or(false);
    let models = normalize_models(
        body.models.unwrap_or_else(|| {
            vec![ProviderModelConfig {
                id: default_model.clone(),
                context_window_tokens: body.context_window_tokens,
                context_output_reserve_ratio: body.context_output_reserve_ratio,
            }]
        }),
        &default_model,
    )?;
    let default_config = models
        .iter()
        .find(|model| model.id == default_model)
        .expect("normalized models contain default");
    let models_json = serialize_models(&models)?;

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    sqlx::query(
        "INSERT INTO llm_providers \
         (id, owner_id, name, kind, base_url, api_key, default_model, \
          context_window_tokens, context_output_reserve_ratio, description, \
          reasoning_passback, models_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&name)
    .bind(&kind)
    .bind(&base_url)
    .bind(&api_key)
    .bind(&default_model)
    .bind(default_config.context_window_tokens)
    .bind(default_config.context_output_reserve_ratio)
    .bind(&description)
    .bind(if reasoning_passback { 1_i64 } else { 0_i64 })
    .bind(&models_json)
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to create provider"))?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("provider vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let sql = format!(
        "SELECT {PROVIDER_COLUMNS} FROM llm_providers \
         WHERE owner_id = ? AND status = 'active' \
         ORDER BY created_at DESC, id DESC"
    );
    let rows = sqlx::query_as::<_, ProviderRow>(&sql)
        .bind(&owner_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(ProviderResponse::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let provider_id = validate_uuid(&provider_id, "provider id")?;
    let row = load_active_owned(state.db.pool(), &provider_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<ProviderResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let provider_id = validate_uuid(&provider_id, "provider id")?;
    let existing = load_active_owned(state.db.pool(), &provider_id, &owner_id).await?;

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    let kind = match body.kind {
        Some(ref raw) => validate_kind(raw)?,
        None => existing.kind.clone(),
    };
    let base_url = match body.base_url {
        Some(ref provided) => normalize_nullable_text(provided.as_deref()),
        None => existing.base_url.clone(),
    };
    let api_key = match body.api_key {
        Some(Some(ref raw)) if !raw.trim().is_empty() => validate_api_key(raw)?,
        _ => existing.api_key.clone(),
    };
    let default_model = match body.default_model {
        Some(Some(ref raw)) => validate_default_model(raw)?,
        _ => existing.default_model.clone(),
    };
    let context_window_tokens = match body.context_window_tokens {
        Some(value) => {
            validate_context_window(value)?;
            value
        }
        None => existing.context_window_tokens,
    };
    let context_output_reserve_ratio = match body.context_output_reserve_ratio {
        Some(value) => {
            validate_reserve_ratio(value)?;
            value
        }
        None => existing.context_output_reserve_ratio,
    };
    let description = match body.description {
        Some(ref provided) => normalize_nullable_text(provided.as_deref()),
        None => existing.description.clone(),
    };
    let reasoning_passback = body
        .reasoning_passback
        .unwrap_or(existing.reasoning_passback != 0);
    let mut models = match body.models.as_ref() {
        Some(models) => normalize_models(models.clone(), &default_model)?,
        None => stored_models(&existing),
    };
    if !models.iter().any(|model| model.id == default_model) {
        models.push(ProviderModelConfig {
            id: default_model.clone(),
            context_window_tokens,
            context_output_reserve_ratio,
        });
    }
    if body.models.is_none() {
        if let Some(model) = models.iter_mut().find(|model| model.id == default_model) {
            model.context_window_tokens = context_window_tokens;
            model.context_output_reserve_ratio = context_output_reserve_ratio;
        }
    }
    let models = normalize_models(models, &default_model)?;
    let default_config = models
        .iter()
        .find(|model| model.id == default_model)
        .expect("normalized models contain default");
    let context_window_tokens = default_config.context_window_tokens;
    let context_output_reserve_ratio = default_config.context_output_reserve_ratio;
    let models_json = serialize_models(&models)?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE llm_providers SET \
         name = ?, kind = ?, base_url = ?, api_key = ?, default_model = ?, \
         context_window_tokens = ?, context_output_reserve_ratio = ?, description = ?, \
         reasoning_passback = ?, models_json = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&kind)
    .bind(&base_url)
    .bind(&api_key)
    .bind(&default_model)
    .bind(context_window_tokens)
    .bind(context_output_reserve_ratio)
    .bind(&description)
    .bind(if reasoning_passback { 1_i64 } else { 0_i64 })
    .bind(&models_json)
    .bind(&now)
    .bind(&provider_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update provider"))?;

    let row = fetch_row(state.db.pool(), &provider_id)
        .await?
        .ok_or_else(|| ApiError::internal("provider vanished after update"))?;
    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let provider_id = validate_uuid(&provider_id, "provider id")?;
    load_active_owned(state.db.pool(), &provider_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE llm_providers SET status = 'deleted', updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&provider_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete provider"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<ModelInfo>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let provider_id = validate_uuid(&provider_id, "provider id")?;
    let provider = load_active_owned(state.db.pool(), &provider_id, &owner_id).await?;
    let config = ProviderConfig {
        kind: provider.kind,
        base_url: provider.base_url,
        api_key: provider.api_key,
        default_model: provider.default_model,
        reasoning_passback: provider.reasoning_passback != 0,
        context_window_tokens: provider.context_window_tokens,
        context_output_reserve_ratio: provider.context_output_reserve_ratio,
    };
    Ok(Json(discover_provider_models(config).await?))
}

pub async fn discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DiscoverRequest>,
) -> Result<Json<Vec<ModelInfo>>, ApiError> {
    current_user_id(&headers, &state.auth.secret_key)?;
    let config = ProviderConfig {
        kind: validate_kind(&body.kind)?,
        base_url: normalize_nullable_text(body.base_url.as_deref()),
        api_key: validate_api_key(&body.api_key)?,
        default_model: body
            .default_model
            .as_deref()
            .map(validate_default_model)
            .transpose()?
            .unwrap_or_default(),
        reasoning_passback: false,
        context_window_tokens: None,
        context_output_reserve_ratio: None,
    };
    Ok(Json(discover_provider_models(config).await?))
}

async fn discover_provider_models(config: ProviderConfig) -> Result<Vec<ModelInfo>, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(MODEL_CATALOG_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .build()
        .map_err(|_| ApiError::internal("failed to create provider model discovery client"))?;
    let models = discover_models(&client, &config)
        .await
        .map_err(model_catalog_api_error)?;
    Ok(models)
}

fn model_catalog_api_error(error: ModelCatalogError) -> ApiError {
    let status = if matches!(error, ModelCatalogError::Timeout { .. }) {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    ApiError::new(status, "provider_model_discovery_failed", error.to_string())
}

async fn load_active_owned(
    pool: &SqlitePool,
    provider_id: &str,
    owner_id: &str,
) -> Result<ProviderRow, ApiError> {
    let row = fetch_row(pool, provider_id)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    if row.status == "deleted" {
        return Err(ApiError::not_found("provider not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "provider belongs to another user",
        ));
    }
    Ok(row)
}

async fn fetch_row(pool: &SqlitePool, provider_id: &str) -> Result<Option<ProviderRow>, ApiError> {
    let sql = format!("SELECT {PROVIDER_COLUMNS} FROM llm_providers WHERE id = ?");
    sqlx::query_as::<_, ProviderRow>(&sql)
        .bind(provider_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
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

fn validate_kind(raw: &str) -> Result<String, ApiError> {
    let kind = raw.trim();
    if VALID_KINDS.contains(&kind) {
        Ok(kind.to_string())
    } else {
        Err(ApiError::invalid_input("unsupported provider kind"))
    }
}

fn validate_api_key(raw: &str) -> Result<String, ApiError> {
    let key = raw.trim();
    if key.is_empty() {
        return Err(ApiError::invalid_input("api_key must not be empty"));
    }
    Ok(key.to_string())
}

fn validate_default_model(raw: &str) -> Result<String, ApiError> {
    let model = raw.trim();
    let len = model.chars().count();
    if !(1..=200).contains(&len) {
        return Err(ApiError::invalid_input(
            "default_model must be between 1 and 200 characters",
        ));
    }
    Ok(model.to_string())
}

fn validate_context_window(value: Option<i64>) -> Result<(), ApiError> {
    if matches!(value, Some(v) if v <= 0) {
        return Err(ApiError::invalid_input(
            "context_window_tokens must be greater than 0",
        ));
    }
    Ok(())
}

fn validate_reserve_ratio(value: Option<f64>) -> Result<(), ApiError> {
    if matches!(value, Some(v) if !(v > 0.0 && v < 1.0)) {
        return Err(ApiError::invalid_input(
            "context_output_reserve_ratio must be greater than 0 and less than 1",
        ));
    }
    Ok(())
}

fn normalize_models(
    models: Vec<ProviderModelConfig>,
    default_model: &str,
) -> Result<Vec<ProviderModelConfig>, ApiError> {
    if models.is_empty() {
        return Err(ApiError::invalid_input(
            "models must contain at least one model",
        ));
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(models.len());
    for model in models {
        let id = validate_default_model(&model.id)?;
        validate_context_window(model.context_window_tokens)?;
        validate_reserve_ratio(model.context_output_reserve_ratio)?;
        if !seen.insert(id.clone()) {
            return Err(ApiError::invalid_input("model ids must be unique"));
        }
        normalized.push(ProviderModelConfig { id, ..model });
    }
    if !seen.contains(default_model) {
        return Err(ApiError::invalid_input(
            "default_model must be present in models",
        ));
    }
    Ok(normalized)
}

fn stored_models(row: &ProviderRow) -> Vec<ProviderModelConfig> {
    row.models_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<ProviderModelConfig>>(raw).ok())
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| {
            vec![ProviderModelConfig {
                id: row.default_model.clone(),
                context_window_tokens: row.context_window_tokens,
                context_output_reserve_ratio: row.context_output_reserve_ratio,
            }]
        })
}

fn serialize_models(models: &[ProviderModelConfig]) -> Result<String, ApiError> {
    serde_json::to_string(models)
        .map_err(|_| ApiError::internal("failed to serialize provider models"))
}

fn normalize_nullable_text(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn mask_api_key(api_key: &str) -> String {
    if api_key.chars().count() <= 4 {
        return "****".to_string();
    }
    let suffix: String = api_key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("****{suffix}")
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
