//! Remote model catalog discovery for saved LLM providers.
//!
//! Requests are built entirely in the backend so provider credentials never
//! cross the API boundary. Errors deliberately retain only the provider kind
//! and, when available, the upstream HTTP status.

use std::{collections::BTreeMap, error::Error, fmt, time::Duration};

use reqwest::{Client, RequestBuilder, Url};
use serde::Serialize;
use serde_json::Value;
use tokio::time::timeout;

use super::ProviderConfig;

pub const MODEL_CATALOG_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_MODEL_CATALOG_BYTES: usize = 2 * 1024 * 1024;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// A sanitized model discovery failure.
///
/// These variants intentionally do not retain request URLs, response bodies,
/// or `reqwest` errors because any of those could contain provider secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCatalogError {
    MissingBaseUrl { kind: String },
    InvalidBaseUrl { kind: String },
    UnsupportedProvider { kind: String },
    RequestFailed { kind: String },
    Timeout { kind: String },
    UpstreamStatus { kind: String, status: u16 },
    ResponseTooLarge { kind: String },
    MalformedResponse { kind: String },
}

impl fmt::Display for ModelCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBaseUrl { kind } => write!(
                formatter,
                "Provider model discovery for {kind} has no base URL configured."
            ),
            Self::InvalidBaseUrl { kind } => write!(
                formatter,
                "Provider model discovery for {kind} has an invalid base URL."
            ),
            Self::UnsupportedProvider { kind } => write!(
                formatter,
                "Provider model discovery does not support provider kind {kind}."
            ),
            Self::RequestFailed { kind } => write!(
                formatter,
                "Provider model discovery for {kind} could not reach the upstream service."
            ),
            Self::Timeout { kind } => {
                write!(formatter, "Provider model discovery for {kind} timed out.")
            }
            Self::UpstreamStatus { kind, status } => write!(
                formatter,
                "Provider model discovery for {kind} was rejected by the upstream service ({status})."
            ),
            Self::ResponseTooLarge { kind } => write!(
                formatter,
                "Provider model discovery for {kind} exceeded the 2 MiB response limit."
            ),
            Self::MalformedResponse { kind } => write!(
                formatter,
                "Provider model discovery for {kind} returned a malformed response."
            ),
        }
    }
}

impl Error for ModelCatalogError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogKind {
    OpenAi,
    Anthropic,
    Gemini,
}

/// Fetch and normalize the configured provider's current model catalog.
pub async fn discover_models(
    client: &Client,
    config: &ProviderConfig,
) -> Result<Vec<ModelInfo>, ModelCatalogError> {
    let kind = config.kind.clone();
    match timeout(MODEL_CATALOG_TIMEOUT, discover_models_inner(client, config)).await {
        Ok(result) => result,
        Err(_) => Err(ModelCatalogError::Timeout { kind }),
    }
}

async fn discover_models_inner(
    client: &Client,
    config: &ProviderConfig,
) -> Result<Vec<ModelInfo>, ModelCatalogError> {
    let catalog_kind = catalog_kind(config)?;
    let request = build_request(client, config, catalog_kind)?;
    let mut response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            ModelCatalogError::Timeout {
                kind: config.kind.clone(),
            }
        } else {
            ModelCatalogError::RequestFailed {
                kind: config.kind.clone(),
            }
        }
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(ModelCatalogError::UpstreamStatus {
            kind: config.kind.clone(),
            status: status.as_u16(),
        });
    }

    if response
        .content_length()
        .is_some_and(|length| length > MAX_MODEL_CATALOG_BYTES as u64)
    {
        return Err(ModelCatalogError::ResponseTooLarge {
            kind: config.kind.clone(),
        });
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            ModelCatalogError::Timeout {
                kind: config.kind.clone(),
            }
        } else {
            ModelCatalogError::RequestFailed {
                kind: config.kind.clone(),
            }
        }
    })? {
        if body.len().saturating_add(chunk.len()) > MAX_MODEL_CATALOG_BYTES {
            return Err(ModelCatalogError::ResponseTooLarge {
                kind: config.kind.clone(),
            });
        }
        body.extend_from_slice(&chunk);
    }

    let payload = serde_json::from_slice::<Value>(&body).map_err(|_| {
        ModelCatalogError::MalformedResponse {
            kind: config.kind.clone(),
        }
    })?;
    let models = parse_models(&payload, catalog_kind).ok_or_else(|| {
        ModelCatalogError::MalformedResponse {
            kind: config.kind.clone(),
        }
    })?;

    let models = normalize_models(models, &config.default_model);
    if models.iter().any(|model| {
        reflects_credential(&model.id, &config.api_key)
            || reflects_credential(&model.name, &config.api_key)
    }) {
        return Err(ModelCatalogError::MalformedResponse {
            kind: config.kind.clone(),
        });
    }

    Ok(models)
}

fn catalog_kind(config: &ProviderConfig) -> Result<CatalogKind, ModelCatalogError> {
    match config.kind.as_str() {
        "openai-compatible" => Ok(CatalogKind::OpenAi),
        "anthropic" | "anthropic-compatible" => Ok(CatalogKind::Anthropic),
        "gemini" => Ok(CatalogKind::Gemini),
        _ => Err(ModelCatalogError::UnsupportedProvider {
            kind: config.kind.clone(),
        }),
    }
}

fn build_request(
    client: &Client,
    config: &ProviderConfig,
    catalog_kind: CatalogKind,
) -> Result<RequestBuilder, ModelCatalogError> {
    let url = catalog_url(config, catalog_kind)?;
    let request = match catalog_kind {
        CatalogKind::OpenAi => client.get(url).bearer_auth(&config.api_key),
        CatalogKind::Anthropic => client
            .get(url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION),
        CatalogKind::Gemini => client.get(url),
    };
    Ok(request)
}

fn catalog_url(
    config: &ProviderConfig,
    catalog_kind: CatalogKind,
) -> Result<Url, ModelCatalogError> {
    let base_url = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(match config.kind.as_str() {
            "anthropic" => Some(ANTHROPIC_BASE_URL),
            "gemini" => Some(GEMINI_BASE_URL),
            _ => None,
        })
        .ok_or_else(|| ModelCatalogError::MissingBaseUrl {
            kind: config.kind.clone(),
        })?;
    let mut url = Url::parse(base_url).map_err(|_| ModelCatalogError::InvalidBaseUrl {
        kind: config.kind.clone(),
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(ModelCatalogError::InvalidBaseUrl {
            kind: config.kind.clone(),
        });
    }

    url.set_fragment(None);
    let base_path = url.path().trim_end_matches('/');
    let path = match catalog_kind {
        CatalogKind::OpenAi => format!("{base_path}/models"),
        CatalogKind::Anthropic => format!("{base_path}/v1/models"),
        CatalogKind::Gemini => {
            let versioned_path = if base_path.ends_with("/v1beta") {
                base_path.to_string()
            } else {
                format!("{base_path}/v1beta")
            };
            format!("{versioned_path}/models")
        }
    };
    url.set_path(&path);
    if catalog_kind == CatalogKind::Gemini {
        let existing_query: Vec<_> = url
            .query_pairs()
            .filter(|(name, _)| name != "key")
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        url.set_query(None);
        url.query_pairs_mut().extend_pairs(existing_query);
        url.query_pairs_mut().append_pair("key", &config.api_key);
    }
    Ok(url)
}

fn parse_models(payload: &Value, catalog_kind: CatalogKind) -> Option<Vec<ModelInfo>> {
    let entries = match catalog_kind {
        CatalogKind::OpenAi | CatalogKind::Anthropic => payload.get("data")?.as_array()?,
        CatalogKind::Gemini => payload.get("models")?.as_array()?,
    };

    match catalog_kind {
        CatalogKind::OpenAi | CatalogKind::Anthropic => entries
            .iter()
            .map(|entry| parse_model(entry, catalog_kind))
            .collect(),
        CatalogKind::Gemini => Some(
            entries
                .iter()
                .filter_map(|entry| parse_model(entry, catalog_kind))
                .collect(),
        ),
    }
}

fn parse_model(entry: &Value, catalog_kind: CatalogKind) -> Option<ModelInfo> {
    if catalog_kind == CatalogKind::Gemini {
        if let Some(methods) = entry.get("supportedGenerationMethods") {
            let supports_generate_content = methods
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == "generateContent"));
            if !supports_generate_content {
                return None;
            }
        }
    }

    let raw_id = entry.get("id").and_then(Value::as_str).or_else(|| {
        (catalog_kind == CatalogKind::Gemini)
            .then(|| entry.get("name").and_then(Value::as_str))
            .flatten()
    })?;
    let id = if catalog_kind == CatalogKind::Gemini {
        raw_id.strip_prefix("models/").unwrap_or(raw_id)
    } else {
        raw_id
    }
    .trim();
    if id.is_empty() {
        return None;
    }

    let name = match catalog_kind {
        CatalogKind::OpenAi => entry
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| entry.get("display_name").and_then(Value::as_str)),
        CatalogKind::Anthropic => entry
            .get("display_name")
            .and_then(Value::as_str)
            .or_else(|| entry.get("name").and_then(Value::as_str)),
        CatalogKind::Gemini => entry.get("displayName").and_then(Value::as_str),
    }
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .unwrap_or(id);

    Some(ModelInfo {
        id: id.to_string(),
        name: name.to_string(),
    })
}

fn normalize_models(models: Vec<ModelInfo>, default_model: &str) -> Vec<ModelInfo> {
    let mut by_id = BTreeMap::new();
    for model in models {
        by_id.entry(model.id.clone()).or_insert(model);
    }

    let default_model = default_model.trim();
    if !default_model.is_empty() {
        by_id
            .entry(default_model.to_string())
            .or_insert_with(|| ModelInfo {
                id: default_model.to_string(),
                name: default_model.to_string(),
            });
    }

    let mut models: Vec<_> = by_id.into_values().collect();
    models.sort_by(|left, right| {
        left.id
            .to_lowercase()
            .cmp(&right.id.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    models
}

fn reflects_credential(value: &str, credential: &str) -> bool {
    !credential.is_empty() && value.contains(credential)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(kind: &str, base_url: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            kind: kind.to_string(),
            base_url: base_url.map(str::to_string),
            api_key: "catalog-secret".to_string(),
            headers: Default::default(),
            user_agent: None,
            default_model: "saved-default".to_string(),
            reasoning_passback: false,
            context_window_tokens: None,
            context_output_reserve_ratio: None,
        }
    }

    #[test]
    fn native_catalogs_use_canonical_base_urls_when_absent() {
        let anthropic = config("anthropic", None);
        assert_eq!(
            catalog_url(&anthropic, CatalogKind::Anthropic)
                .unwrap()
                .as_str(),
            "https://api.anthropic.com/v1/models"
        );

        let gemini = config("gemini", None);
        assert_eq!(
            catalog_url(&gemini, CatalogKind::Gemini).unwrap().as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models?key=catalog-secret"
        );
    }

    #[test]
    fn compatible_catalogs_still_require_explicit_base_urls() {
        for (kind, catalog_kind) in [
            ("openai-compatible", CatalogKind::OpenAi),
            ("anthropic-compatible", CatalogKind::Anthropic),
        ] {
            let error = catalog_url(&config(kind, None), catalog_kind).unwrap_err();
            assert_eq!(
                error,
                ModelCatalogError::MissingBaseUrl {
                    kind: kind.to_string()
                }
            );
        }
    }
}
